use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use session_core::config::{parse_config, validate};
use session_core::discovery::{
    example_config_json, load_from_candidates, unique_paths, LoadOutcome,
};
use session_core::files::{build_list_cmd, join_path, parse_listing};
use session_core::session::SessionState;

use crate::runner::{run_session, RunnerMsg};

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
    for dir in [exe_dir.as_deref(), cwd.as_deref()].into_iter().flatten() {
        candidates.push(dir.join("examples").join("config.example.json"));
    }

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
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(b"__close".to_vec()).map_err(|e| e.to_string())
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

/// 列远端目录。走一次性 ssh（`build_exec_argv`），不占用也不污染任何 PTY 会话。
///
/// 必须 `spawn_blocking`：`std::process::Command::output()` 是阻塞调用，
/// 直接在 async 上下文里跑会卡住整个运行时（面板转圈时连事件都发不出去）。
///
/// 返回的 `dir` 是远端解析后的规范路径，前端**不要**自己拼 `~` 或 `..` 的上一级 ——
/// 想看上一层就再传一次 `<dir>/..`，让远端去解析。
#[tauri::command]
pub async fn list_dir(
    state: State<'_, AppState>,
    host: String,
    dir: String,
) -> Result<DirView, String> {
    // 锁只在这一小段里持有——MutexGuard 不是 Send，跨 await 会编译不过
    let target = {
        let guard = state.config.lock().unwrap();
        let cfg = guard.as_ref().ok_or("config not loaded")?;
        cfg.to_ssh_target(&host)
            .ok_or_else(|| format!("unknown host: {host}"))?
    };

    let argv = session_core::command::build_exec_argv(&target, &build_list_cmd(&dir));

    let output = tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new(&argv[0]).args(&argv[1..]).output()
    })
    .await
    .map_err(|e| format!("列目录任务失败: {e}"))?
    .map_err(|e| format!("执行 ssh 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        return Err(if msg.is_empty() {
            format!("ssh 退出码 {:?}", output.status.code())
        } else {
            msg.to_string()
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
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
