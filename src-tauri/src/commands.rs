use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use session_core::agents::{build_probe_agents_cmd, parse_probe_agents_output, KNOWN_AGENTS};
use session_core::command::{build_exec_argv, login_shell, probe_outcome, SshTarget};
use session_core::config::{parse_config, resolve_target, validate};
use session_core::discovery::{
    example_config_json, load_from_candidates, unique_paths, LoadOutcome,
};
use session_core::files::{build_list_cmd, join_path, parse_listing};
use session_core::reconnect::CLOSE_FRAME;
use session_core::session::SessionState;
use session_core::ssh_config::parse_ssh_config;

use crate::filechan::FileChan;
use crate::runner::{hide_console, run_session, RunnerMsg};

#[derive(Serialize)]
pub struct HostView {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub extra_ssh_args: Vec<String>,
}

#[derive(Serialize)]
pub struct AgentView {
    pub id: String,
    pub label: String,
    pub cmd: String,
}

#[derive(Serialize)]
pub struct ConfigView {
    pub hosts: Vec<HostView>,
    pub agents: Vec<AgentView>,
    pub source_path: String,
    pub searched: Vec<String>,
}

/// 本机 `~/.ssh/config` 里发现的一个 `Host` 别名。
///
/// `host_name`/`user`/`port`/`proxy_jump` 仅供界面展示（按钮 tooltip）；
/// 真正连的是 `alias` 本身，由系统 ssh 去解释 —— 见 `session_core::ssh_config`。
#[derive(Serialize)]
pub struct SshHostView {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub proxy_jump: Option<String>,
}

/// Structured config-load failure so the UI can react (and guide the user)
/// instead of being left with a silently empty screen.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigError {
    NotFound { searched: Vec<String> },
    Invalid { path: String, errors: Vec<String> },
}

pub struct AppState {
    pub config: Mutex<Option<session_core::config::Config>>,
    pub inputs: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    pub runners: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    /// 文件面板共用的那条 ssh（见 filechan.rs）。换 host 或空闲超时会重建。
    pub file_chan: Arc<Mutex<Option<FileChan>>>,
}

/// 走长驻通道跑一条远端命令。通道不存在 / 换了 host / 断了就重建。
///
/// 必须 `spawn_blocking`：通道是阻塞读写，直接在 async 上下文里跑会卡住运行时
/// （和原先 `Command::output()` 一样的坑）。
async fn exec_remote(
    state: &State<'_, AppState>,
    host: &str,
    cmd: String,
) -> Result<String, String> {
    let target = {
        let guard = state.config.lock().unwrap();
        let cfg = guard.as_ref().ok_or("config not loaded")?;
        cfg.to_ssh_target(host)
            .ok_or_else(|| format!("unknown host: {host}"))?
    };

    let chan = state.file_chan.clone();
    let host = host.to_string();
    tauri::async_runtime::spawn_blocking(move || with_chan(&chan, &target, &host, &cmd))
        .await
        .map_err(|e| format!("远端命令任务失败: {e}"))?
}

fn with_chan(
    chan: &Arc<Mutex<Option<FileChan>>>,
    target: &SshTarget,
    host: &str,
    cmd: &str,
) -> Result<String, String> {
    match once(chan, target, host, cmd) {
        Ok(out) => Ok(out),
        // 通道可能已经死了（远端断网、命令超时留下半截输出）：丢掉重建再来一次。
        // 两条路都失败时，报第一次的错 —— 它更可能是真正的原因。
        Err(first) => {
            chan.lock().unwrap().take();
            once(chan, target, host, cmd).map_err(|_| first)
        }
    }
}

fn once(
    chan: &Arc<Mutex<Option<FileChan>>>,
    target: &SshTarget,
    host: &str,
    cmd: &str,
) -> Result<String, String> {
    let mut guard = chan.lock().unwrap();
    if guard.as_ref().map_or(true, |c| !c.usable(host)) {
        // 旧通道在这里被替换掉，Drop 会 kill 掉它的 ssh
        *guard = Some(FileChan::spawn(target, host)?);
    }
    guard.as_mut().unwrap().run(cmd)
}

/// 一次性 ssh 跑一条远端命令，返回 stdout。
///
/// 用在「问一句就回」、而且**不该占用文件面板那条长驻通道**的场景（探测已装 agent）。
/// 独立 ssh 进程 + `ConnectTimeout=10` 兜底，不碰 `state.file_chan`，所以既不会把
/// 文件面板的通道顶掉，两边也不会互相等锁。
///
/// 这是「一次性」的：单条命令最长约 `ConnectTimeout` + 命令本身，不可取消。
/// 探测是有进度提示的用户动作，这个上限够用；哪天要更长或要可取消，得换成
/// 自己持 Child 句柄的形式（`runner.rs` 的会话进程就是那么做的）。
///
/// 必须 `spawn_blocking`：`Command::output()` 是阻塞的，直接在 async 上下文里
/// 跑会卡住运行时（和 `exec_remote` 同一个坑）。
async fn exec_remote_oneshot(target: SshTarget, cmd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let argv = build_exec_argv(&target, &cmd);
        let mut command = StdCommand::new(&argv[0]);
        command.args(&argv[1..]);
        let out = hide_console(&mut command)
            .output()
            .map_err(|e| format!("启动 ssh 失败: {e}"))?;
        probe_outcome(
            out.status.code().unwrap_or(-1),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        )
    })
    .await
    .map_err(|e| format!("远端命令任务失败: {e}"))?
}

/// Candidate config locations, highest priority first. The first existing file
/// wins. The user config directory is where "generate example config" writes,
/// so a packaged app works without a file next to the executable.
fn config_candidates(app: &AppHandle, override_path: Option<&str>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(path) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(path));
    }

    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let app_dir = app.path().app_config_dir().ok();

    for dir in [cwd.as_deref(), exe_dir.as_deref(), app_dir.as_deref()]
        .into_iter()
        .flatten()
    {
        candidates.push(dir.join("config.json"));
    }
    // 不再把 `examples/config.example.json` 当候选：那是「给人复制的模板」，
    // 里面的 REPLACE_WITH_* 占位符会通过校验，于是变成一个看着能用、点了
    // 必然连不上的主机。样例文件本身仍保留给用户手工参考。

    unique_paths(candidates)
}

fn display_paths(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

#[tauri::command]
pub fn load_config(
    app: AppHandle,
    state: State<AppState>,
    path: Option<String>,
) -> Result<ConfigView, ConfigError> {
    let candidates = config_candidates(&app, path.as_deref());

    match load_from_candidates(&candidates) {
        LoadOutcome::Loaded { path, config } => {
            let view = ConfigView {
                hosts: config
                    .hosts
                    .iter()
                    .map(|h| HostView {
                        name: h.name.clone(),
                        host: h.host.clone(),
                        user: h.user.clone(),
                        extra_ssh_args: h.extra_ssh_args.clone(),
                    })
                    .collect(),
                agents: config
                    .agents
                    .iter()
                    .map(|a| AgentView {
                        id: a.id.clone(),
                        label: a.label.clone(),
                        cmd: a.cmd.clone(),
                    })
                    .collect(),
                source_path: path.display().to_string(),
                searched: display_paths(&candidates),
            };
            *state.config.lock().unwrap() = Some(config);
            // 同 save_config：换了配置就让长驻通道按新的 target 重建
            state.file_chan.lock().unwrap().take();
            Ok(view)
        }
        LoadOutcome::NotFound { .. } => Err(ConfigError::NotFound {
            searched: display_paths(&candidates),
        }),
        LoadOutcome::Invalid { path, errors } => Err(ConfigError::Invalid {
            path: path.display().to_string(),
            errors,
        }),
    }
}

/// Write the first-run template into the user config directory (no-op if a
/// config already exists there). Returns the path so the UI can show it.
#[tauri::command]
pub fn write_example_config(app: AppHandle) -> Result<String, String> {
    let path = user_config_path(&app)?;
    if !path.exists() {
        std::fs::write(&path, example_config_json())
            .map_err(|e| format!("写入失败 {}: {e}", path.display()))?;
    }
    Ok(path.display().to_string())
}

fn user_config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法解析用户配置目录: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败 {}: {e}", dir.display()))?;
    Ok(dir.join("config.json"))
}

/// Validate and save a config JSON from the in-app editor. Always writes to the
/// user config directory (the same place "generate example config" writes), so
/// a config picked up from the exe directory or cwd gets imported there on
/// first save. On success the parsed config replaces the in-memory state and
/// the written path is returned; on failure the field-level errors come back
/// as the existing `ConfigError::Invalid` payload.
#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<AppState>,
    json: String,
) -> Result<String, ConfigError> {
    let cfg = parse_config(&json).map_err(|e| ConfigError::Invalid {
        path: "(编辑内容)".to_string(),
        errors: vec![e],
    })?;
    let path = user_config_path(&app).map_err(|e| ConfigError::Invalid {
        path: "(用户配置目录)".to_string(),
        errors: vec![e],
    })?;
    validate(&cfg).map_err(|errors| ConfigError::Invalid {
        path: path.display().to_string(),
        errors,
    })?;
    let fail = |e: String| ConfigError::Invalid {
        path: path.display().to_string(),
        errors: vec![e],
    };
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| fail(e.to_string()))?;
    std::fs::write(&path, text + "\n").map_err(|e| fail(format!("写入失败: {e}")))?;
    *state.config.lock().unwrap() = Some(cfg);
    // host / key 可能变了，长驻通道握的是旧 target，丢掉让它按需重建
    state.file_chan.lock().unwrap().take();
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn start_session(
    app: AppHandle,
    state: State<AppState>,
    host: String,
    agent: String,
    project: String,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone().ok_or("config not loaded")?;
    let target = cfg.to_ssh_target(&host).ok_or_else(|| format!("unknown host: {host}"))?;
    let agent_cmd = cfg
        .agents
        .iter()
        .find(|a| a.id == agent)
        .ok_or_else(|| format!("unknown agent: {agent}"))?
        .cmd
        .clone();

    // 会话名按 agent 区分（设计文档：session: hermes / harness / <project>）。
    // 若用 host 名，同一 host 上切换 agent 会命中同一个 tmux 会话：`-A` 会直接
    // 附身到旧 agent，而不是启动新 agent。
    let session_name = if project.trim().is_empty() {
        agent.clone()
    } else {
        format!("{agent}-{project}")
    };
    let id = session_name.clone();

    // C1: 拒绝重复会话——否则旧 runner 的输入通道被覆盖断开，其默认 kill
    // 会杀掉同一 tmux 会话，连带杀死新会话
    if state.runners.lock().unwrap().contains_key(&id) {
        return Err(format!("session already running: {id}"));
    }

    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<RunnerMsg>();
    state.inputs.lock().unwrap().insert(id.clone(), in_tx);

    // I1: 关闭标签只断开本地 ssh（§7.5）；远端 tmux 会话仅在用户显式
    // close_session(kill_remote=true) 时才被终止
    let kill_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.runners.lock().unwrap().insert(id.clone(), kill_flag.clone());

    let app_for_thread = app.clone();
    let id_for_thread = id.clone();
    std::thread::spawn(move || {
        run_session(msg_tx, target, session_name, agent_cmd, 80, 24, in_rx, kill_flag);
    });

    // 消息泵：把 RunnerMsg 转成前端事件；runner 结束（msg_rx 关闭）后清理会话表
    let inputs_for_thread = state.inputs.clone();
    let runners_for_thread = state.runners.clone();
    let id_for_cleanup = id_for_thread.clone();
    std::thread::spawn(move || {
        for msg in msg_rx {
            match msg {
                RunnerMsg::Output(data) => {
                    let _ = app_for_thread.emit("session-output", serde_json::json!({ "id": id_for_thread, "data": data }));
                }
                RunnerMsg::State(s) => {
                    let _ = app_for_thread.emit("session-state", serde_json::json!({ "id": id_for_thread, "state": state_name(s) }));
                }
                RunnerMsg::Notice(n) => {
                    let _ = app_for_thread.emit("session-notice", serde_json::json!({ "id": id_for_thread, "message": n }));
                }
            }
        }
        inputs_for_thread.lock().unwrap().remove(&id_for_cleanup);
        runners_for_thread.lock().unwrap().remove(&id_for_cleanup);
    });

    Ok(id)
}

fn state_name(s: SessionState) -> &'static str {
    match s {
        SessionState::Connecting => "connecting",
        SessionState::Connected => "connected",
        SessionState::Retrying => "retrying",
        SessionState::Exited => "exited",
        SessionState::Closed => "closed",
    }
}

#[tauri::command]
pub fn write_session(state: State<AppState>, id: String, data: String) -> Result<(), String> {
    // 拦下 Ctrl+C / Ctrl+D / Ctrl+Z / Ctrl+\ —— 误触会让远端 agent 退出，
    // 其 tmux 会话随之销毁，就再也接不回原来的会话了。
    let forward = session_core::keys::strip_quit_keys(&data);
    if forward.is_empty() {
        return Ok(());
    }
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(forward.into_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_session(state: State<AppState>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(format!("__resize:{cols}x{rows}").into_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_session(state: State<AppState>, id: String, kill_remote: bool) -> Result<(), String> {
    if let Some(flag) = state.runners.lock().unwrap().get(&id) {
        flag.store(kill_remote, std::sync::atomic::Ordering::SeqCst);
    }
    {
        let guard = state.inputs.lock().unwrap();
        let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
        tx.send(CLOSE_FRAME.to_vec()).map_err(|e| e.to_string())?;
    }

    // runner 的收尾是异步的：它要先 kill PTY、wait 子进程，消息泵线程才清会话表。
    // 不等它清完就返回，用户马上重开同一个会话会撞上 start_session 的重复保护
    // （"session already running"）—— 表现就是"关掉之后再也连不上"。
    // 锁必须在等待前释放，否则 runner 的清理线程拿不到 inputs 锁。
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.runners.lock().unwrap().contains_key(&id) {
        if Instant::now() >= deadline {
            // 兜底：真卡住了也不能把前端挂死，让它照旧报 already running
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct EntryView {
    pub name: String,
    /// 绝对路径，前端拖拽时直接拿去插进终端
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Serialize)]
pub struct DirView {
    /// 远端 `pwd` 解析出的规范绝对路径（`~` / `..` 都已展开），面包屑用它
    pub dir: String,
    pub entries: Vec<EntryView>,
}

/// 列远端目录。走文件面板的长驻 ssh 通道（见 filechan.rs），
/// 不占用也不污染任何 PTY 会话。
///
/// 返回的 `dir` 是远端解析后的规范路径，前端**不要**自己拼 `~` 或 `..` 的上一级 ——
/// 想看上一层就再传一次 `<dir>/..`，让远端去解析。
#[tauri::command]
pub async fn list_dir(
    state: State<'_, AppState>,
    host: String,
    dir: String,
) -> Result<DirView, String> {
    let stdout = exec_remote(&state, &host, build_list_cmd(&dir)).await?;
    let listing = parse_listing(&stdout);
    let entries = listing
        .entries
        .into_iter()
        .map(|e| EntryView {
            path: join_path(&listing.dir, &e.name),
            name: e.name,
            is_dir: e.is_dir,
            size: e.size,
        })
        .collect();
    Ok(DirView { dir: listing.dir, entries })
}

/// 探测某台主机上装了哪些已知 agent。
///
/// `host` 既可以是已保存配置里的 `host.name`，也可以是 `~/.ssh/config` 里的别名 ——
/// 后者是首次启动的情况：那时还没有配置文件，界面上列的正是别名
/// （见 `resolve_target`）。以前这里只认已保存的 host 名，首启因此探测不了，
/// 只能把注册表全集猜着写进配置。
///
/// 走一次性 ssh（`exec_remote_oneshot`），不占用文件面板那条长驻通道。
/// 返回的只是候选：写不写进配置由界面上确认。
///
/// 探测命令套在登录 shell 里跑 —— **必须与会话（`build_remote_tmux_cmd`）同一个
/// 形状**，否则「探测说没装、会话其实能跑」两边又不一致，首启引导会一直拒绝
/// 写配置。见 `command::login_shell`。
#[tauri::command]
pub async fn probe_agents(
    state: State<'_, AppState>,
    host: String,
) -> Result<Vec<AgentView>, String> {
    // 作用域必须先结束：std 的 MutexGuard 不能跨 await 持有
    let target = {
        let guard = state.config.lock().unwrap();
        resolve_target(guard.as_ref(), &host)
    };
    let cmd = login_shell(&build_probe_agents_cmd());
    let stdout = exec_remote_oneshot(target, cmd).await?;
    Ok(parse_probe_agents_output(&stdout)
        .into_iter()
        .map(|a| AgentView {
            id: a.id.to_string(),
            label: a.label.to_string(),
            cmd: a.cmd.to_string(),
        })
        .collect())
}

/// 本机 ssh 客户端配置的路径（Windows 上即 `%USERPROFILE%\.ssh\config`）。
///
/// 用 Tauri 的 `home_dir()`（内部已封装 `dirs::home_dir`），不为这一处引入 `dirs` crate。
fn ssh_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let home = app
        .path()
        .home_dir()
        .map_err(|e| format!("无法解析用户主目录: {e}"))?;
    Ok(home.join(".ssh").join("config"))
}

/// 列出本机 `~/.ssh/config` 里的 `Host` 别名，供首次启动时一键导入。
///
/// 文件不存在（或 `~/.ssh` 不存在）返回空列表而不是报错 —— 「没配过 ssh config」
/// 是正常情况，不该让客户端起不来；真正读取失败才报错。
#[tauri::command]
pub fn list_ssh_hosts(app: AppHandle) -> Result<Vec<SshHostView>, String> {
    let path = ssh_config_path(&app)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    Ok(parse_ssh_config(&text)
        .into_iter()
        .map(|h| SshHostView {
            alias: h.alias,
            host_name: h.host_name,
            user: h.user,
            port: h.port,
            proxy_jump: h.proxy_jump,
        })
        .collect())
}

/// 注册表里的已知 agent 模板，供首次启动生成初始配置。
///
/// 从前端调用而不是在 TS 里硬编码，保证与 `probe_agents` 共用同一份事实源。
#[tauri::command]
pub fn known_agents() -> Vec<AgentView> {
    KNOWN_AGENTS
        .iter()
        .map(|a| AgentView {
            id: a.id.to_string(),
            label: a.label.to_string(),
            cmd: a.cmd.to_string(),
        })
        .collect()
}
