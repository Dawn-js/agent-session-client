use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use session_core::config::{parse_config, validate};
use session_core::session::SessionState;

use crate::runner::{run_session, RunnerMsg};

#[derive(Serialize)]
pub struct HostView {
    pub name: String,
    pub host: String,
}

#[derive(Serialize)]
pub struct AgentView {
    pub id: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct ConfigView {
    pub hosts: Vec<HostView>,
    pub agents: Vec<AgentView>,
}

pub struct AppState {
    pub config: Mutex<Option<session_core::config::Config>>,
    pub inputs: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    pub runners: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
}

#[tauri::command]
pub fn load_config(state: State<AppState>, path: String) -> Result<ConfigView, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let cfg = parse_config(&text)?;
    validate(&cfg).map_err(|e| e.join("; "))?;
    let view = ConfigView {
        hosts: cfg.hosts.iter().map(|h| HostView { name: h.name.clone(), host: h.host.clone() }).collect(),
        agents: cfg.agents.iter().map(|a| AgentView { id: a.id.clone(), label: a.label.clone() }).collect(),
    };
    *state.config.lock().unwrap() = Some(cfg);
    Ok(view)
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

    let session_name = if project.trim().is_empty() { host.clone() } else { format!("{agent}-{project}") };
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
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(data.into_bytes()).map_err(|e| e.to_string())
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
