use tauri::Manager;
use commands::agent::AgentGlobalState;
use commands::terminal::TerminalManager;

mod commands;
mod agent;
mod services;

/// 应用运行入口
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AgentGlobalState::new())
        .manage(TerminalManager::new())
        .setup(|app| {
            // 获取主窗口并设置标题
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Agent IDE");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // FS 命令
            commands::fs::read_file_content,
            commands::fs::write_file_content,
            commands::fs::list_directory,
            commands::fs::file_exists,
            // Agent 命令
            commands::agent::get_agent_state,
            commands::agent::send_agent_prompt,
            commands::agent::stop_agent,
            // Terminal 命令
            commands::terminal::spawn_terminal,
            commands::terminal::write_to_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::kill_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Agent IDE");
}
