#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use runtime::{
    CommandEvent, CommandRequest, CommandResult, FileDocument, GitState, RuntimeBootstrap,
    SaveFileRequest, WorkspaceState, WorkspaceTask,
};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    Emitter, Manager,
};

const OPEN_WORKSPACE_ID: &str = "file.open_workspace";
const SAVE_ACTIVE_ID: &str = "file.save_active";
const FOCUS_EXPLORER_ID: &str = "view.focus_explorer";
const FOCUS_EDITOR_ID: &str = "view.focus_editor";
const FOCUS_REVIEW_ID: &str = "view.focus_review";
const FOCUS_LOGS_ID: &str = "view.focus_logs";
const RELOAD_WINDOW_ID: &str = "debug.reload_window";
const OPEN_DEVTOOLS_ID: &str = "debug.open_devtools";
const CLOSE_DEVTOOLS_ID: &str = "debug.close_devtools";

#[derive(serde::Serialize, Clone)]
struct RuntimeLogEvent {
    level: &'static str,
    message: String,
}

#[derive(serde::Serialize, Clone)]
struct CommandStreamEvent {
    execution_id: String,
    stream: &'static str,
    line: String,
}

#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ExecutionEvent {
    Started { id: String, command: String },
    Finished {
        id: String,
        command: String,
        success: bool,
        exit_code: Option<i32>,
    },
}

#[tauri::command]
fn bootstrap_runtime() -> RuntimeBootstrap {
    runtime::bootstrap()
}

#[tauri::command]
fn open_workspace(app: tauri::AppHandle, path: String) -> Result<WorkspaceState, String> {
    let result = runtime::open_workspace(&path);
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(_) => format!("Workspace opened: {}", path),
            Err(err) => format!("Workspace open failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn read_file(app: tauri::AppHandle, root: String, path: String) -> Result<FileDocument, String> {
    let result = runtime::read_file(&root, &path);
    emit_runtime_log(
        &app,
        if result.is_ok() { "info" } else { "error" },
        match &result {
            Ok(_) => format!("File read: {}", path),
            Err(err) => format!("File read failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn save_file(
    app: tauri::AppHandle,
    root: String,
    payload: SaveFileRequest,
) -> Result<(), String> {
    let path = payload.path.clone();
    let result = runtime::save_file(&root, payload);
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(_) => format!("File saved: {}", path),
            Err(err) => format!("File save failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn git_state(root: String) -> GitState {
    runtime::git_state(&root)
}

#[tauri::command]
fn workspace_tasks(root: String) -> Vec<WorkspaceTask> {
    runtime::workspace_tasks(&root)
}

#[tauri::command]
fn run_workspace_command(
    app: tauri::AppHandle,
    root: String,
    payload: CommandRequest,
) -> Result<CommandResult, String> {
    let command_label = payload.command.clone();
    emit_runtime_log(&app, "info", format!("Running command: {}", command_label));

    let mut started = false;
    let result = runtime::run_command_streaming(&root, payload, |event: CommandEvent| {
        if !started {
            emit_execution_event(
                &app,
                ExecutionEvent::Started {
                    id: event.execution_id.clone(),
                    command: command_label.clone(),
                },
            );
            started = true;
        }
        let _ = app.emit(
            "command-stream",
            CommandStreamEvent {
                execution_id: event.execution_id.clone(),
                stream: event.stream,
                line: event.line.clone(),
            },
        );
        emit_runtime_log(
            &app,
            if event.stream == "stderr" { "error" } else { "info" },
            format!("[{}] {}", event.stream, event.line),
        );
    });

    match &result {
        Ok(output) => {
            if !started {
                emit_execution_event(
                    &app,
                    ExecutionEvent::Started {
                        id: output.execution_id.clone(),
                        command: output.command.clone(),
                    },
                );
            }
            emit_execution_event(
                &app,
                ExecutionEvent::Finished {
                    id: output.execution_id.clone(),
                    command: output.command.clone(),
                    success: output.success,
                    exit_code: output.exit_code,
                },
            );
            let exit_code = output
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "terminated".into());
            emit_runtime_log(
                &app,
                if output.success { "success" } else { "error" },
                format!("Command finished ({}) with exit code {}", output.command, exit_code),
            );
        }
        Err(err) => {
            emit_runtime_log(&app, "error", format!("Command failed to start: {}", err));
        }
    }

    result
}

fn emit_runtime_log(app: &tauri::AppHandle, level: &'static str, message: String) {
    let _ = app.emit(
        "runtime-log",
        RuntimeLogEvent {
            level,
            message,
        },
    );
}

fn emit_menu_action(app: &tauri::AppHandle, action: &str) {
    let _ = app.emit("menu-action", action);
}

fn emit_execution_event(app: &tauri::AppHandle, event: ExecutionEvent) {
    let _ = app.emit("execution-event", event);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let open_workspace_item = MenuItemBuilder::with_id(OPEN_WORKSPACE_ID, "Open Folder...")
                .accelerator("CmdOrCtrl+O")
                .build(app)?;
            let save_active_item = MenuItemBuilder::with_id(SAVE_ACTIVE_ID, "Save")
                .accelerator("CmdOrCtrl+S")
                .build(app)?;
            let explorer_item = MenuItemBuilder::with_id(FOCUS_EXPLORER_ID, "Explorer")
                .accelerator("CmdOrCtrl+Shift+E")
                .build(app)?;
            let editor_item = MenuItemBuilder::with_id(FOCUS_EDITOR_ID, "Editor").build(app)?;
            let review_item = MenuItemBuilder::with_id(FOCUS_REVIEW_ID, "Review").build(app)?;
            let logs_item = MenuItemBuilder::with_id(FOCUS_LOGS_ID, "Logs")
                .accelerator("CmdOrCtrl+Shift+L")
                .build(app)?;
            let reload_item = MenuItemBuilder::with_id(RELOAD_WINDOW_ID, "Reload Window")
                .accelerator("CmdOrCtrl+R")
                .build(app)?;
            let open_devtools_item = MenuItemBuilder::with_id(OPEN_DEVTOOLS_ID, "Open DevTools")
                .accelerator("CmdOrCtrl+Shift+I")
                .build(app)?;
            let close_devtools_item =
                MenuItemBuilder::with_id(CLOSE_DEVTOOLS_ID, "Close DevTools").build(app)?;

            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&open_workspace_item)
                .item(&save_active_item)
                .separator()
                .item(&PredefinedMenuItem::quit(app, None)?)
                .build()?;

            let view_menu = SubmenuBuilder::new(app, "View")
                .item(&explorer_item)
                .item(&editor_item)
                .item(&review_item)
                .item(&logs_item)
                .separator()
                .item(&reload_item)
                .build()?;

            let debug_menu = SubmenuBuilder::new(app, "Debug")
                .item(&open_devtools_item)
                .item(&close_devtools_item)
                .build()?;

            let help_menu = SubmenuBuilder::new(app, "Help")
                .item(&PredefinedMenuItem::about(
                    app,
                    Some("Agent IDE".into()),
                    None,
                )?)
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .item(&view_menu)
                .item(&debug_menu)
                .item(&help_menu)
                .build()?;
            app.set_menu(menu)?;

            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            if let Some(window) = app.get_webview_window("main") {
                match event.id().as_ref() {
                    OPEN_WORKSPACE_ID => emit_menu_action(app, "open-workspace"),
                    SAVE_ACTIVE_ID => emit_menu_action(app, "save-active"),
                    FOCUS_EXPLORER_ID => emit_menu_action(app, "focus-explorer"),
                    FOCUS_EDITOR_ID => emit_menu_action(app, "focus-editor"),
                    FOCUS_REVIEW_ID => emit_menu_action(app, "focus-review"),
                    FOCUS_LOGS_ID => emit_menu_action(app, "focus-logs"),
                    RELOAD_WINDOW_ID => {
                        let _ = window.eval("window.location.reload()");
                    }
                    OPEN_DEVTOOLS_ID => window.open_devtools(),
                    CLOSE_DEVTOOLS_ID => window.close_devtools(),
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_runtime,
            open_workspace,
            read_file,
            save_file,
            git_state,
            workspace_tasks,
            run_workspace_command
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop shell");
}
