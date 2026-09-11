#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod runner;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState {
            config: Mutex::new(None),
            inputs: Arc::new(Mutex::new(HashMap::new())),
            runners: Arc::new(Mutex::new(HashMap::new())),
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::start_session,
            commands::write_session,
            commands::resize_session,
            commands::close_session,
            commands::write_example_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
