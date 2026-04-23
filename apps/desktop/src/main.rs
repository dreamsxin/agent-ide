#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use runtime::{
    AgentPlan, AgentPlanSummary, AgentProviderConfig, AgentProviderStatus, AgentTaskRequest,
    CommandEvent, CommandRequest, CommandResult, CommandStarted, FileDocument, GitState,
    RuntimeBootstrap, SaveFileRequest, WorkspaceState, WorkspaceTask,
};
use std::{collections::HashMap, sync::Mutex};
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

struct ExecutionRegistry(Mutex<HashMap<String, u32>>);

struct AgentPlanStore(Mutex<HashMap<String, AgentPlan>>);

#[derive(serde::Deserialize)]
struct AgentStepStatusRequest {
    root: String,
    plan_id: String,
    step_id: String,
    status: String,
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
    Cancelled { id: String },
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
fn decompose_agent_task(
    app: tauri::AppHandle,
    store: tauri::State<AgentPlanStore>,
    root: String,
    payload: AgentTaskRequest,
) -> Result<AgentPlan, String> {
    let result = runtime::decompose_agent_task(&root, payload);
    if let Ok(plan) = &result {
        if let Ok(mut plans) = store.0.lock() {
            plans.insert(plan.id.clone(), plan.clone());
        }
        if let Err(err) = runtime::save_agent_plan(&root, plan) {
            emit_runtime_log(&app, "error", format!("Agent plan save failed: {}", err));
        }
    }
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(plan) => format!("Agent task decomposed: {}", plan.goal),
            Err(err) => format!("Agent task decomposition failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn latest_agent_plan(
    app: tauri::AppHandle,
    store: tauri::State<AgentPlanStore>,
    root: String,
) -> Result<Option<AgentPlan>, String> {
    let result = runtime::latest_agent_plan(&root);
    if let Ok(Some(plan)) = &result {
        if let Ok(mut plans) = store.0.lock() {
            plans.insert(plan.id.clone(), plan.clone());
        }
        emit_runtime_log(&app, "info", format!("Loaded agent plan: {}", plan.id));
    }
    result
}

#[tauri::command]
fn agent_plan_history(root: String) -> Result<Vec<AgentPlanSummary>, String> {
    runtime::agent_plan_history(&root)
}

#[tauri::command]
fn read_agent_plan(
    store: tauri::State<AgentPlanStore>,
    root: String,
    plan_id: String,
) -> Result<AgentPlan, String> {
    let plan = runtime::read_agent_plan(&root, &plan_id)?;
    if let Ok(mut plans) = store.0.lock() {
        plans.insert(plan.id.clone(), plan.clone());
    }
    Ok(plan)
}

#[tauri::command]
fn agent_provider_status(root: String) -> Result<AgentProviderStatus, String> {
    runtime::agent_provider_status(&root)
}

#[tauri::command]
fn save_agent_provider_config(
    app: tauri::AppHandle,
    root: String,
    payload: AgentProviderConfig,
) -> Result<AgentProviderStatus, String> {
    let result = runtime::save_agent_provider_config(&root, payload);
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(status) => format!("Agent provider set: {}", status.provider),
            Err(err) => format!("Agent provider update failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn save_agent_provider_secret(
    app: tauri::AppHandle,
    root: String,
    payload: runtime::AgentProviderSecretRequest,
) -> Result<AgentProviderStatus, String> {
    let result = runtime::save_agent_provider_secret(&root, payload);
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(status) => format!("Agent provider secret configured: {}", status.provider),
            Err(err) => format!("Agent provider secret update failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn clear_agent_provider_secret(
    app: tauri::AppHandle,
    root: String,
) -> Result<AgentProviderStatus, String> {
    let result = runtime::clear_agent_provider_secret(&root);
    emit_runtime_log(
        &app,
        if result.is_ok() { "success" } else { "error" },
        match &result {
            Ok(status) => format!("Agent provider secret cleared: {}", status.provider),
            Err(err) => format!("Agent provider secret clear failed: {}", err),
        },
    );
    result
}

#[tauri::command]
fn update_agent_step_status(
    app: tauri::AppHandle,
    store: tauri::State<AgentPlanStore>,
    payload: AgentStepStatusRequest,
) -> Result<AgentPlan, String> {
    let mut plans = store.0.lock().map_err(|err| err.to_string())?;
    let plan = plans
        .get_mut(&payload.plan_id)
        .ok_or_else(|| "agent plan not found".to_string())?;
    let step = plan
        .steps
        .iter_mut()
        .find(|step| step.id == payload.step_id)
        .ok_or_else(|| "agent plan step not found".to_string())?;

    match payload.status.as_str() {
        "pending" | "active" | "done" => {
            step.status = payload.status;
        }
        _ => return Err("invalid agent step status".into()),
    }

    emit_runtime_log(
        &app,
        "info",
        format!("Agent step updated: {} -> {}", payload.step_id, step.status),
    );

    runtime::save_agent_plan(&payload.root, plan)?;

    Ok(plan.clone())
}

#[tauri::command]
fn run_workspace_command(
    app: tauri::AppHandle,
    registry: tauri::State<ExecutionRegistry>,
    root: String,
    payload: CommandRequest,
) -> Result<CommandResult, String> {
    let command_label = payload.command.clone();
    emit_runtime_log(&app, "info", format!("Running command: {}", command_label));

    let result = runtime::run_command_streaming(
        &root,
        payload,
        |started: CommandStarted| {
            if let Ok(mut executions) = registry.0.lock() {
                executions.insert(started.execution_id.clone(), started.pid);
            }
            emit_execution_event(
                &app,
                ExecutionEvent::Started {
                    id: started.execution_id,
                    command: started.command,
                },
            );
        },
        |event: CommandEvent| {
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
        },
    );

    match &result {
        Ok(output) => {
            if let Ok(mut executions) = registry.0.lock() {
                executions.remove(&output.execution_id);
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

#[tauri::command]
fn cancel_execution(
    app: tauri::AppHandle,
    registry: tauri::State<ExecutionRegistry>,
    execution_id: String,
) -> Result<(), String> {
    let pid = {
        let mut executions = registry.0.lock().map_err(|err| err.to_string())?;
        executions.remove(&execution_id)
    };

    let Some(pid) = pid else {
        return Err("execution is not running".into());
    };

    let result = runtime::cancel_process(pid);
    match &result {
        Ok(_) => {
            emit_execution_event(
                &app,
                ExecutionEvent::Cancelled {
                    id: execution_id.clone(),
                },
            );
            emit_runtime_log(&app, "info", format!("Execution cancelled: {}", execution_id));
        }
        Err(err) => {
            emit_runtime_log(&app, "error", format!("Cancel failed: {}", err));
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
        .manage(ExecutionRegistry(Mutex::new(HashMap::new())))
        .manage(AgentPlanStore(Mutex::new(HashMap::new())))
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
            decompose_agent_task,
            latest_agent_plan,
            agent_plan_history,
            read_agent_plan,
            agent_provider_status,
            save_agent_provider_config,
            save_agent_provider_secret,
            clear_agent_provider_secret,
            update_agent_step_status,
            run_workspace_command,
            cancel_execution
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop shell");
}
