use tauri::Manager;
use commands::agent::AgentGlobalState;
use commands::fs::FileWatcherState;
use commands::lsp::LspManager;
use commands::terminal::TerminalManager;

mod commands;
pub mod agent;
pub mod services;

/// 应用运行入口
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AgentGlobalState::new())
        .manage(TerminalManager::new())
        .manage(FileWatcherState::new())
        .manage(LspManager::new())
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
            commands::fs::delete_path,
            commands::fs::create_file,
            commands::fs::create_directory,
            commands::fs::rename_path,
            commands::fs::reveal_in_file_explorer,
            commands::fs::copy_path,
            commands::fs::get_file_metadata,
            commands::fs::search_files,
            commands::fs::watch_start,
            commands::fs::watch_stop,
            // Agent 命令
            commands::agent::get_agent_state,
            commands::agent::send_agent_prompt,
            commands::agent::stop_agent,
            commands::agent::set_agent_mode,
            commands::agent::apply_diffs,
            commands::agent::apply_diff,
            commands::agent::apply_diff_hunk,
            commands::agent::reject_diffs,
            commands::agent::reject_diff,
            commands::agent::reject_diff_hunk,
            commands::agent::get_agent_steps,
            commands::agent::get_agent_diffs,
            commands::agent::update_llm_config,
            commands::agent::get_llm_config,
            commands::agent::save_llm_profile,
            commands::agent::set_active_llm_profile,
            commands::agent::delete_llm_profile,
            commands::agent::set_context_compression,
            commands::agent::set_active_role,
            commands::agent::get_active_role,
            commands::agent::get_pipeline,
            commands::agent::update_pipeline,
            commands::agent::reset_pipeline,
            commands::agent::test_llm_connection,
            commands::agent::save_workspace_path,
            commands::agent::get_workspace_path,
            // Git 命令
            commands::git::git_status,
            commands::git::git_diff,
            commands::git::git_commit,
            commands::git::git_stage_files,
            commands::git::git_unstage_files,
            commands::git::git_discard_files,
            commands::git::git_checkout_branch,
            commands::git::git_checkout_remote_branch,
            commands::git::git_fetch,
            commands::git::git_pull,
            commands::git::git_push,
            commands::git::git_resolve_conflict,
            // Tasks 命令
            commands::tasks::discover_project_tasks,
            commands::tasks::run_project_task,
            // LSP 命令
            commands::lsp::lsp_initialize,
            commands::lsp::lsp_open_file,
            commands::lsp::lsp_change_file,
            commands::lsp::lsp_hover,
            commands::lsp::lsp_definition,
            commands::lsp::lsp_completion,
            commands::lsp::lsp_document_symbols,
            commands::lsp::lsp_rename,
            commands::lsp::lsp_code_actions,
            commands::lsp::lsp_status,
            // Terminal 命令
            commands::terminal::spawn_terminal,
            commands::terminal::write_to_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::kill_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Agent IDE");
}
