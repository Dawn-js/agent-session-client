#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod filechan;
mod runner;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// `app.state()` 是 Manager trait 的方法，trait 必须在作用域内
use tauri::Manager;

fn main() {
    let app = tauri::Builder::default()
        .manage(commands::AppState {
            config: Mutex::new(None),
            inputs: Arc::new(Mutex::new(HashMap::new())),
            runners: Arc::new(Mutex::new(HashMap::new())),
            file_chan: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::start_session,
            commands::write_session,
            commands::resize_session,
            commands::close_session,
            commands::write_example_config,
            commands::save_config,
            commands::list_dir,
            commands::list_skills,
            commands::probe_agents,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // 退出时关掉文件面板那条常驻 ssh：进程退出不会替我们收子进程，
        // 不 kill 它就会留在后台（ledger 第 2 条：ssh 必须显式 kill + wait）。
        if let tauri::RunEvent::Exit = event {
            app.state::<commands::AppState>()
                .file_chan
                .lock()
                .unwrap()
                .take();
        }
    });
}
