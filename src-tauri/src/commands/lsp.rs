use crate::services::workspace;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub struct LspManager {
    inner: Arc<LspInner>,
}

struct LspInner {
    server: Mutex<Option<LspServer>>,
    next_id: AtomicU64,
}

struct LspServer {
    child: Child,
    stdin: ChildStdin,
    pending: HashMap<u64, mpsc::Sender<Value>>,
    workspace_root: PathBuf,
    language_id: String,
    language_name: String,
    executable: PathBuf,
    server_source: String,
    install_command: String,
    workspace_config_files: Vec<String>,
    indexing_status: String,
    indexing_message: String,
    opened_documents: usize,
    change_count: usize,
    diagnostics_count: usize,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub file: String,
    pub range: LspRange,
    pub severity: String,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspDiagnosticsEvent {
    pub file: String,
    pub diagnostics: Vec<LspDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspStatusEvent {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspStatusSnapshot {
    pub status: String,
    pub message: String,
    #[serde(rename = "workspaceRoot")]
    pub workspace_root: Option<String>,
    #[serde(rename = "languageId")]
    pub language_id: String,
    #[serde(rename = "languageName")]
    pub language_name: String,
    #[serde(rename = "serverPath")]
    pub server_path: Option<String>,
    #[serde(rename = "serverSource")]
    pub server_source: Option<String>,
    #[serde(rename = "installCommand")]
    pub install_command: String,
    #[serde(rename = "workspaceConfigFiles")]
    pub workspace_config_files: Vec<String>,
    #[serde(rename = "indexingStatus")]
    pub indexing_status: String,
    #[serde(rename = "indexingMessage")]
    pub indexing_message: String,
    #[serde(rename = "openedDocuments")]
    pub opened_documents: usize,
    #[serde(rename = "changeCount")]
    pub change_count: usize,
    #[serde(rename = "diagnosticsCount")]
    pub diagnostics_count: usize,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
}

#[tauri::command]
pub fn lsp_probe(
    workspace_path: Option<String>,
    language_id: Option<String>,
) -> Result<LspStatusSnapshot, String> {
    let root = match workspace_path {
        Some(path) if !path.trim().is_empty() => workspace::resolve_existing(&path)?,
        _ => workspace::workspace_root()?,
    };
    let root = workspace::shell_compatible_path(root);
    let spec = lsp_spec(language_id.as_deref().unwrap_or("typescript"))?;
    let executable = find_language_server(&root, &spec);
    let workspace_config_files = detect_lsp_workspace_config_files(&root, &spec);
    let (indexing_status, indexing_message) =
        infer_indexing_state(&root, &workspace_config_files, &spec);
    let install_command = spec.install_command.to_string();
    let status = if executable.is_some() {
        "available"
    } else {
        "unavailable"
    };
    let message = if executable.is_some() {
        format!(
            "{} executable was found but is not initialized.",
            spec.display_name
        )
    } else {
        format!(
            "{} was not found. Install with: {}",
            spec.server_name, install_command
        )
    };

    Ok(LspStatusSnapshot {
        status: status.to_string(),
        message,
        workspace_root: Some(root.to_string_lossy().to_string()),
        language_id: spec.language_id.to_string(),
        language_name: spec.display_name.to_string(),
        server_path: executable
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        server_source: executable
            .as_ref()
            .map(|path| classify_lsp_server_source(&root, path, &spec)),
        install_command,
        workspace_config_files,
        indexing_status,
        indexing_message,
        opened_documents: 0,
        change_count: 0,
        diagnostics_count: 0,
        last_error: None,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct LspHover {
    pub contents: String,
    pub range: Option<LspRange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspLocation {
    pub file: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCompletionItem {
    pub label: String,
    pub kind: Option<u64>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    #[serde(rename = "insertText")]
    pub insert_text: Option<String>,
    #[serde(rename = "sortText")]
    pub sort_text: Option<String>,
    #[serde(rename = "filterText")]
    pub filter_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspDocumentSymbol {
    pub name: String,
    pub kind: u64,
    pub range: LspRange,
    #[serde(rename = "selectionRange")]
    pub selection_range: LspRange,
    pub children: Vec<LspDocumentSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspTextEdit {
    pub file: String,
    pub range: LspRange,
    #[serde(rename = "newText")]
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspWorkspaceEdit {
    pub edits: Vec<LspTextEdit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCodeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edit: Option<LspWorkspaceEdit>,
}

#[derive(Debug, Deserialize)]
pub struct LspTextDocumentRequest {
    pub file: String,
    pub content: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: Option<u64>,
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(LspInner {
                server: Mutex::new(None),
                next_id: AtomicU64::new(1),
            }),
        }
    }
}

#[tauri::command]
pub fn lsp_initialize(
    app: AppHandle,
    manager: tauri::State<'_, LspManager>,
    workspace_path: Option<String>,
    language_id: Option<String>,
) -> Result<(), String> {
    let root = match workspace_path {
        Some(path) if !path.trim().is_empty() => workspace::resolve_existing(&path)?,
        _ => workspace::workspace_root()?,
    };
    let root = workspace::shell_compatible_path(root);
    let spec = lsp_spec(language_id.as_deref().unwrap_or("typescript"))?;
    {
        let server = manager.inner.server.lock().map_err(|e| e.to_string())?;
        if server
            .as_ref()
            .map(|server| server.workspace_root == root && server.language_id == spec.language_id)
            .unwrap_or(false)
        {
            return Ok(());
        }
    }

    let install_command = spec.install_command.to_string();
    let executable = find_language_server(&root, &spec).ok_or_else(|| {
        format!(
            "Start {} failed: {} was not found. Install with: {}",
            spec.display_name, spec.server_name, install_command
        )
    })?;
    let server_source = classify_lsp_server_source(&root, &executable, &spec);
    let workspace_config_files = detect_lsp_workspace_config_files(&root, &spec);
    let (indexing_status, indexing_message) =
        infer_indexing_state(&root, &workspace_config_files, &spec);
    let mut child = Command::new(&executable)
        .arg("--stdio")
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Start {} failed: {}. Executable: {}",
                spec.display_name,
                e,
                executable.display()
            )
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} stdin is unavailable", spec.display_name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} stdout is unavailable", spec.display_name))?;

    {
        let mut server = manager.inner.server.lock().map_err(|e| e.to_string())?;
        *server = Some(LspServer {
            child,
            stdin,
            pending: HashMap::new(),
            workspace_root: root.clone(),
            language_id: spec.language_id.to_string(),
            language_name: spec.display_name.to_string(),
            executable: executable.clone(),
            server_source,
            install_command,
            workspace_config_files,
            indexing_status,
            indexing_message,
            opened_documents: 0,
            change_count: 0,
            diagnostics_count: 0,
            last_error: None,
        });
    }

    let state = Arc::clone(&manager.inner);
    std::thread::spawn(move || read_lsp_output(app, state, stdout));

    let root_uri = path_to_uri(&root);
    let initialize = request(
        &manager,
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": { "dynamicRegistration": false, "contentFormat": ["markdown", "plaintext"] },
                    "definition": { "dynamicRegistration": false, "linkSupport": false },
                    "completion": { "dynamicRegistration": false, "completionItem": { "documentationFormat": ["markdown", "plaintext"] } },
                    "documentSymbol": { "dynamicRegistration": false, "hierarchicalDocumentSymbolSupport": true },
                    "rename": { "dynamicRegistration": false, "prepareSupport": false },
                    "codeAction": { "dynamicRegistration": false, "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["quickfix", "refactor", "source", "source.organizeImports"] } } },
                    "publishDiagnostics": { "relatedInformation": true, "versionSupport": true },
                    "synchronization": { "dynamicRegistration": false, "willSave": false, "willSaveWaitUntil": false, "didSave": false }
                },
                "workspace": {
                    "applyEdit": false,
                    "configuration": true,
                    "workspaceFolders": true
                }
            },
            "workspaceFolders": [
                { "uri": root_uri, "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace") }
            ]
        }),
    )?;
    if initialize.get("error").is_some() {
        return Err(format!(
            "{} initialize failed: {}",
            spec.display_name, initialize
        ));
    }
    notify(&manager, "initialized", json!({}))?;
    Ok(())
}

#[tauri::command]
pub fn lsp_open_file(
    manager: tauri::State<'_, LspManager>,
    request: LspTextDocumentRequest,
) -> Result<(), String> {
    let file = workspace::resolve_existing(&request.file)?;
    update_server_stats(&manager, |server| {
        server.opened_documents = server.opened_documents.saturating_add(1);
    });
    notify(
        &manager,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": path_to_uri(&file),
                "languageId": request.language_id,
                "version": request.version.unwrap_or(1),
                "text": request.content
            }
        }),
    )
}

#[tauri::command]
pub fn lsp_change_file(
    manager: tauri::State<'_, LspManager>,
    request: LspTextDocumentRequest,
) -> Result<(), String> {
    let file = workspace::resolve_for_write(&request.file)?;
    update_server_stats(&manager, |server| {
        server.change_count = server.change_count.saturating_add(1);
    });
    notify(
        &manager,
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": path_to_uri(&file),
                "version": request.version.unwrap_or(2)
            },
            "contentChanges": [{ "text": request.content }]
        }),
    )
}

#[tauri::command]
pub fn lsp_hover(
    manager: tauri::State<'_, LspManager>,
    file: String,
    line: u32,
    character: u32,
) -> Result<Option<LspHover>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "position": { "line": line, "character": character }
        }),
    )?;
    Ok(response.get("result").and_then(parse_hover))
}

#[tauri::command]
pub fn lsp_definition(
    manager: tauri::State<'_, LspManager>,
    file: String,
    line: u32,
    character: u32,
) -> Result<Vec<LspLocation>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "position": { "line": line, "character": character }
        }),
    )?;
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    Ok(parse_locations(result))
}

#[tauri::command]
pub fn lsp_completion(
    manager: tauri::State<'_, LspManager>,
    file: String,
    line: u32,
    character: u32,
) -> Result<Vec<LspCompletionItem>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": 1 }
        }),
    )?;
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    Ok(parse_completion_items(result))
}

#[tauri::command]
pub fn lsp_document_symbols(
    manager: tauri::State<'_, LspManager>,
    file: String,
) -> Result<Vec<LspDocumentSymbol>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": path_to_uri(&file) } }),
    )?;
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    Ok(parse_document_symbols(result))
}

#[tauri::command]
pub fn lsp_rename(
    manager: tauri::State<'_, LspManager>,
    file: String,
    line: u32,
    character: u32,
    new_name: String,
) -> Result<Option<LspWorkspaceEdit>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/rename",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "position": { "line": line, "character": character },
            "newName": new_name
        }),
    )?;
    Ok(response.get("result").and_then(parse_workspace_edit))
}

#[tauri::command]
pub fn lsp_code_actions(
    manager: tauri::State<'_, LspManager>,
    file: String,
    range: LspRange,
    diagnostics: Vec<LspDiagnostic>,
) -> Result<Vec<LspCodeAction>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "range": range,
            "context": {
                "diagnostics": diagnostics.into_iter().map(lsp_diagnostic_to_protocol).collect::<Vec<_>>()
            }
        }),
    )?;
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    Ok(parse_code_actions(result))
}

#[tauri::command]
pub fn lsp_status(manager: tauri::State<'_, LspManager>) -> Result<LspStatusSnapshot, String> {
    let guard = manager.inner.server.lock().map_err(|e| e.to_string())?;
    let Some(server) = guard.as_ref() else {
        return Ok(LspStatusSnapshot {
            status: "unavailable".to_string(),
            message: "Language server is not initialized.".to_string(),
            workspace_root: None,
            language_id: "unknown".to_string(),
            language_name: "Language Server".to_string(),
            server_path: None,
            server_source: None,
            install_command:
                "Open a TypeScript/JavaScript, Go, Python, or Rust file to see install guidance."
                    .to_string(),
            workspace_config_files: Vec::new(),
            indexing_status: "unavailable".to_string(),
            indexing_message: "Start a language server to validate workspace indexing.".to_string(),
            opened_documents: 0,
            change_count: 0,
            diagnostics_count: 0,
            last_error: None,
        });
    };

    Ok(LspStatusSnapshot {
        status: "ready".to_string(),
        message: format!("{} ready.", server.language_name),
        workspace_root: Some(server.workspace_root.to_string_lossy().to_string()),
        language_id: server.language_id.clone(),
        language_name: server.language_name.clone(),
        server_path: Some(server.executable.to_string_lossy().to_string()),
        server_source: Some(server.server_source.clone()),
        install_command: server.install_command.clone(),
        workspace_config_files: server.workspace_config_files.clone(),
        indexing_status: server.indexing_status.clone(),
        indexing_message: server.indexing_message.clone(),
        opened_documents: server.opened_documents,
        change_count: server.change_count,
        diagnostics_count: server.diagnostics_count,
        last_error: server.last_error.clone(),
    })
}

fn request(manager: &LspManager, method: &str, params: Value) -> Result<Value, String> {
    let id = manager.inner.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    {
        let mut guard = manager.inner.server.lock().map_err(|e| e.to_string())?;
        let server = guard
            .as_mut()
            .ok_or_else(|| "Language server is not initialized".to_string())?;
        server.pending.insert(id, tx);
        write_message(
            &mut server.stdin,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )?;
    }
    rx.recv_timeout(Duration::from_secs(8))
        .map_err(|_| format!("LSP request timed out: {}", method))
}

fn notify(manager: &LspManager, method: &str, params: Value) -> Result<(), String> {
    let mut guard = manager.inner.server.lock().map_err(|e| e.to_string())?;
    let server = guard
        .as_mut()
        .ok_or_else(|| "Language server is not initialized".to_string())?;
    write_message(
        &mut server.stdin,
        &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    )
}

fn write_message(stdin: &mut ChildStdin, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    write!(
        stdin,
        "Content-Length: {}\r\n\r\n{}",
        body.as_bytes().len(),
        body
    )
    .map_err(|e| format!("Write LSP message: {}", e))?;
    stdin.flush().map_err(|e| format!("Flush LSP: {}", e))
}

fn read_lsp_output(app: AppHandle, state: Arc<LspInner>, stdout: std::process::ChildStdout) {
    let mut reader = BufReader::new(stdout);
    loop {
        let message = match read_message(&mut reader) {
            Ok(Some(message)) => message,
            Ok(None) => {
                let name = current_language_name(&state);
                set_server_error(&state, &format!("{} stdout closed.", name));
                let _ = app.emit(
                    "lsp-status",
                    LspStatusEvent {
                        status: "error".to_string(),
                        message: format!("{} stdout closed.", name),
                    },
                );
                break;
            }
            Err(error) => {
                set_server_error(&state, &error);
                let _ = app.emit(
                    "lsp-status",
                    LspStatusEvent {
                        status: "error".to_string(),
                        message: error,
                    },
                );
                break;
            }
        };
        let method = message.get("method").and_then(Value::as_str);
        if let Some(id) = message
            .get("id")
            .and_then(Value::as_u64)
            .filter(|_| method.is_none())
        {
            if let Ok(mut guard) = state.server.lock() {
                if let Some(server) = guard.as_mut() {
                    if let Some(tx) = server.pending.remove(&id) {
                        let _ = tx.send(message);
                    }
                }
            }
            continue;
        }
        if let (Some(id), Some(method)) = (message.get("id"), method) {
            handle_server_request(&state, id.clone(), method, message.get("params").cloned());
            continue;
        }
        if method == Some("textDocument/publishDiagnostics") {
            if let Some(params) = message.get("params") {
                let diagnostics = parse_diagnostics(params);
                update_server_stats_inner(&state, |server| {
                    server.diagnostics_count = server
                        .diagnostics_count
                        .saturating_add(diagnostics.diagnostics.len());
                });
                let _ = app.emit("lsp-diagnostics", diagnostics);
            }
        }
    }
}

fn update_server_stats<F>(manager: &LspManager, update: F)
where
    F: FnOnce(&mut LspServer),
{
    if let Ok(mut guard) = manager.inner.server.lock() {
        if let Some(server) = guard.as_mut() {
            update(server);
        }
    }
}

fn update_server_stats_inner<F>(state: &Arc<LspInner>, update: F)
where
    F: FnOnce(&mut LspServer),
{
    if let Ok(mut guard) = state.server.lock() {
        if let Some(server) = guard.as_mut() {
            update(server);
        }
    }
}

fn set_server_error(state: &Arc<LspInner>, error: &str) {
    update_server_stats_inner(state, |server| {
        server.last_error = Some(error.to_string());
    });
}

fn current_language_name(state: &Arc<LspInner>) -> String {
    state
        .server
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|server| server.language_name.clone()))
        .unwrap_or_else(|| "Language server".to_string())
}

fn handle_server_request(state: &Arc<LspInner>, id: Value, method: &str, params: Option<Value>) {
    let result = match method {
        "workspace/configuration" => lsp_configuration_response(params.as_ref()),
        "workspace/workspaceFolders" => {
            let folders = state
                .server
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|server| server.workspace_root.clone()))
                .map(|root| {
                    json!([{ "uri": path_to_uri(&root), "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace") }])
                })
                .unwrap_or_else(|| json!([]));
            folders
        }
        _ => Value::Null,
    };

    if let Ok(mut guard) = state.server.lock() {
        if let Some(server) = guard.as_mut() {
            let _ = write_message(
                &mut server.stdin,
                &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            );
        }
    }
}

fn lsp_configuration_response(params: Option<&Value>) -> Value {
    let items = params
        .and_then(|params| params.get("items"))
        .and_then(Value::as_array);
    let Some(items) = items else {
        return json!([]);
    };

    Value::Array(
        items
            .iter()
            .map(|item| {
                let section = item
                    .get("section")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match section {
                    "typescript" => json!({
                        "format": { "enable": true },
                        "implementationsCodeLens": { "enabled": false },
                        "referencesCodeLens": { "enabled": false },
                        "inlayHints": {},
                        "preferences": {},
                        "suggest": { "completeFunctionCalls": false },
                        "tsserver": {},
                        "validate": { "enable": true }
                    }),
                    "javascript" => json!({
                        "format": { "enable": true },
                        "implementationsCodeLens": { "enabled": false },
                        "referencesCodeLens": { "enabled": false },
                        "inlayHints": {},
                        "preferences": {},
                        "suggest": { "completeFunctionCalls": false },
                        "tsserver": {},
                        "validate": { "enable": true }
                    }),
                    "python" => json!({
                        "analysis": {
                            "autoImportCompletions": true,
                            "diagnosticMode": "workspace",
                            "typeCheckingMode": "basic"
                        }
                    }),
                    "rust-analyzer" => json!({
                        "cargo": { "allFeatures": true },
                        "checkOnSave": true,
                        "diagnostics": { "enable": true }
                    }),
                    _ => Value::Null,
                }
            })
            .collect(),
    )
}

#[derive(Clone, Copy)]
struct LspSpec {
    language_id: &'static str,
    display_name: &'static str,
    server_name: &'static str,
    binary_name: &'static str,
    fallback_binary_names: &'static [&'static str],
    windows_binary_names: &'static [&'static str],
    install_command: &'static str,
    workspace_markers: &'static [&'static str],
}

fn lsp_spec(language_id: &str) -> Result<LspSpec, String> {
    match language_id {
        "typescript" => Ok(LspSpec {
            language_id: "typescript",
            display_name: "TypeScript LSP",
            server_name: "typescript-language-server",
            binary_name: "typescript-language-server",
            fallback_binary_names: &[],
            windows_binary_names: &[
                "typescript-language-server.cmd",
                "typescript-language-server.exe",
                "typescript-language-server",
            ],
            install_command: "npm install -D typescript typescript-language-server",
            workspace_markers: &["tsconfig.json", "jsconfig.json", "package.json"],
        }),
        "javascript" => Ok(LspSpec {
            language_id: "javascript",
            display_name: "TypeScript LSP",
            server_name: "typescript-language-server",
            binary_name: "typescript-language-server",
            fallback_binary_names: &[],
            windows_binary_names: &[
                "typescript-language-server.cmd",
                "typescript-language-server.exe",
                "typescript-language-server",
            ],
            install_command: "npm install -D typescript typescript-language-server",
            workspace_markers: &["tsconfig.json", "jsconfig.json", "package.json"],
        }),
        "go" => Ok(LspSpec {
            language_id: "go",
            display_name: "Go LSP",
            server_name: "gopls",
            binary_name: "gopls",
            fallback_binary_names: &[],
            windows_binary_names: &["gopls.exe", "gopls.cmd", "gopls"],
            install_command: "go install golang.org/x/tools/gopls@latest",
            workspace_markers: &["go.work", "go.mod"],
        }),
        "python" => Ok(LspSpec {
            language_id: "python",
            display_name: "Python LSP",
            server_name: "pyright-langserver",
            binary_name: "pyright-langserver",
            fallback_binary_names: &["pylsp"],
            windows_binary_names: &[
                "pyright-langserver.cmd",
                "pyright-langserver.exe",
                "pyright-langserver",
                "pylsp.exe",
                "pylsp.cmd",
                "pylsp",
            ],
            install_command:
                "npm install -D pyright (or pip install python-lsp-server for pylsp fallback)",
            workspace_markers: &[
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
                "Pipfile",
                "poetry.lock",
            ],
        }),
        "rust" => Ok(LspSpec {
            language_id: "rust",
            display_name: "Rust LSP",
            server_name: "rust-analyzer",
            binary_name: "rust-analyzer",
            fallback_binary_names: &[],
            windows_binary_names: &["rust-analyzer.exe", "rust-analyzer.cmd", "rust-analyzer"],
            install_command: "rustup component add rust-analyzer",
            workspace_markers: &["Cargo.toml", "Cargo.lock", "rust-project.json"],
        }),
        other => Err(format!(
            "No language server configured for language: {}",
            other
        )),
    }
}

fn read_message(
    reader: &mut BufReader<std::process::ChildStdout>,
) -> Result<Option<Value>, String> {
    let mut header = Vec::new();
    let mut buf = [0u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(None),
            Ok(_) => header.push(buf[0]),
            Err(e) => return Err(format!("Read LSP header: {}", e)),
        }
    }
    let header = String::from_utf8_lossy(&header);
    let length = header
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .ok_or_else(|| "Missing LSP Content-Length".to_string())?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("Read LSP body: {}", e))?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| format!("Parse LSP JSON: {}", e))
}

fn parse_diagnostics(params: &Value) -> LspDiagnosticsEvent {
    let file = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
        .unwrap_or_default();
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LspDiagnostic {
                        file: file.clone(),
                        range: parse_range(item.get("range")?)?,
                        severity: match item.get("severity").and_then(Value::as_u64).unwrap_or(3) {
                            1 => "error",
                            2 => "warning",
                            _ => "info",
                        }
                        .to_string(),
                        message: item.get("message")?.as_str()?.to_string(),
                        source: item
                            .get("source")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    LspDiagnosticsEvent { file, diagnostics }
}

fn lsp_diagnostic_to_protocol(diagnostic: LspDiagnostic) -> Value {
    json!({
        "range": diagnostic.range,
        "severity": match diagnostic.severity.as_str() {
            "error" => 1,
            "warning" => 2,
            "info" => 3,
            _ => 4,
        },
        "source": diagnostic.source,
        "message": diagnostic.message
    })
}

fn parse_hover(value: &Value) -> Option<LspHover> {
    Some(LspHover {
        contents: hover_contents_to_string(value.get("contents")?),
        range: value.get("range").and_then(parse_range),
    })
}

fn hover_contents_to_string(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(value_text) = value.get("value").and_then(Value::as_str) {
        return value_text.to_string();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .map(hover_contents_to_string)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    String::new()
}

fn parse_locations(value: &Value) -> Vec<LspLocation> {
    if value.is_null() {
        return Vec::new();
    }
    if let Some(array) = value.as_array() {
        return array.iter().filter_map(parse_location).collect();
    }
    parse_location(value).into_iter().collect()
}

fn parse_completion_items(value: &Value) -> Vec<LspCompletionItem> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    items
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LspCompletionItem {
                        label: item.get("label")?.as_str()?.to_string(),
                        kind: item.get("kind").and_then(Value::as_u64),
                        detail: item
                            .get("detail")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        documentation: item.get("documentation").and_then(markup_to_string),
                        insert_text: item
                            .get("insertText")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| {
                                item.get("textEdit")
                                    .and_then(|edit| edit.get("newText"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            }),
                        sort_text: item
                            .get("sortText")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        filter_text: item
                            .get("filterText")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_document_symbols(value: &Value) -> Vec<LspDocumentSymbol> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(parse_document_symbol).collect())
        .unwrap_or_default()
}

fn parse_document_symbol(value: &Value) -> Option<LspDocumentSymbol> {
    Some(LspDocumentSymbol {
        name: value.get("name")?.as_str()?.to_string(),
        kind: value.get("kind")?.as_u64()?,
        range: parse_range(value.get("range")?)?,
        selection_range: parse_range(value.get("selectionRange")?)?,
        children: value
            .get("children")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(parse_document_symbol).collect())
            .unwrap_or_default(),
    })
}

fn parse_code_actions(value: &Value) -> Vec<LspCodeAction> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LspCodeAction {
                        title: item.get("title")?.as_str()?.to_string(),
                        kind: item.get("kind").and_then(Value::as_str).map(str::to_string),
                        edit: item.get("edit").and_then(parse_workspace_edit),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_workspace_edit(value: &Value) -> Option<LspWorkspaceEdit> {
    if value.is_null() {
        return None;
    }
    let mut edits = Vec::new();
    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, file_edits) in changes {
            let Some(file) = uri_to_path(uri) else {
                continue;
            };
            if let Some(file_edits) = file_edits.as_array() {
                for edit in file_edits {
                    if let Some(range) = edit.get("range").and_then(parse_range) {
                        if let Some(new_text) = edit.get("newText").and_then(Value::as_str) {
                            edits.push(LspTextEdit {
                                file: file.clone(),
                                range,
                                new_text: new_text.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    if let Some(document_changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            let Some(file) = change
                .get("textDocument")
                .and_then(|document| document.get("uri"))
                .and_then(Value::as_str)
                .and_then(uri_to_path)
            else {
                continue;
            };
            if let Some(file_edits) = change.get("edits").and_then(Value::as_array) {
                for edit in file_edits {
                    if let Some(range) = edit.get("range").and_then(parse_range) {
                        if let Some(new_text) = edit.get("newText").and_then(Value::as_str) {
                            edits.push(LspTextEdit {
                                file: file.clone(),
                                range,
                                new_text: new_text.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Some(LspWorkspaceEdit { edits })
}

fn markup_to_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string).or_else(|| {
        value
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn parse_location(value: &Value) -> Option<LspLocation> {
    let uri = value
        .get("uri")
        .or_else(|| value.get("targetUri"))?
        .as_str()?;
    let range_value = value
        .get("range")
        .or_else(|| value.get("targetSelectionRange"))
        .or_else(|| value.get("targetRange"))?;
    Some(LspLocation {
        file: uri_to_path(uri)?,
        range: parse_range(range_value)?,
    })
}

fn parse_range(value: &Value) -> Option<LspRange> {
    Some(LspRange {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

fn parse_position(value: &Value) -> Option<LspPosition> {
    Some(LspPosition {
        line: value.get("line")?.as_u64()? as u32,
        character: value.get("character")?.as_u64()? as u32,
    })
}

fn path_to_uri(path: &Path) -> String {
    let path = workspace::shell_compatible_path(path.to_path_buf());
    let text = encode_uri_path(&path.to_string_lossy().replace('\\', "/"));
    if cfg!(windows) {
        format!("file:///{}", text)
    } else {
        format!("file://{}", text)
    }
}

fn uri_to_path(uri: &str) -> Option<String> {
    let decoded = uri.strip_prefix("file://")?;
    let path = if cfg!(windows) {
        decoded.trim_start_matches('/')
    } else {
        decoded
    };
    Some(decode_uri_path(path)?.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn encode_uri_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn decode_uri_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = path.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

impl Drop for LspServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn find_language_server(workspace_root: &Path, spec: &LspSpec) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(
        workspace_root
            .join("node_modules")
            .join(".bin")
            .join(platform_server_binary_name(spec)),
    );

    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            for name in spec.windows_binary_names {
                candidates.push(PathBuf::from(&appdata).join("npm").join(name));
            }
        }
        if let Ok(gopath) = std::env::var("GOPATH") {
            for name in spec.windows_binary_names {
                candidates.push(PathBuf::from(&gopath).join("bin").join(name));
            }
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path) {
            if cfg!(windows) {
                for name in spec.windows_binary_names {
                    candidates.push(entry.join(name));
                }
            } else {
                candidates.push(entry.join(spec.binary_name));
                for name in spec.fallback_binary_names {
                    candidates.push(entry.join(name));
                }
            }
        }
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn classify_lsp_server_source(workspace_root: &Path, executable: &Path, spec: &LspSpec) -> String {
    let executable = workspace::shell_compatible_path(executable.to_path_buf());
    let workspace_root = workspace::shell_compatible_path(workspace_root.to_path_buf());
    let workspace_bin = workspace_root.join("node_modules").join(".bin");
    if executable.starts_with(&workspace_bin) {
        "workspace devDependency".to_string()
    } else if spec.language_id == "go" && executable.to_string_lossy().contains("\\go\\bin") {
        "GOPATH bin".to_string()
    } else if spec.language_id == "python"
        && executable
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("python")
    {
        "Python environment".to_string()
    } else if spec.language_id == "rust"
        && executable
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains(".cargo")
    {
        "Cargo bin".to_string()
    } else {
        "global PATH".to_string()
    }
}

fn detect_lsp_workspace_config_files(workspace_root: &Path, spec: &LspSpec) -> Vec<String> {
    spec.workspace_markers
        .iter()
        .filter_map(|name| {
            let path = workspace_root.join(name);
            path.is_file().then(|| (*name).to_string())
        })
        .collect()
}

fn infer_indexing_state(
    workspace_root: &Path,
    config_files: &[String],
    spec: &LspSpec,
) -> (String, String) {
    if spec.language_id == "go" {
        return infer_go_indexing_state(workspace_root, config_files);
    }
    if spec.language_id == "python" {
        return infer_python_indexing_state(workspace_root, config_files);
    }
    if spec.language_id == "rust" {
        return infer_rust_indexing_state(workspace_root, config_files);
    }
    if config_files
        .iter()
        .any(|file| file == "tsconfig.json" || file == "jsconfig.json")
    {
        return (
            "configured".to_string(),
            "Workspace has tsconfig/jsconfig; TypeScript server can index project references and compiler options.".to_string(),
        );
    }
    if config_files.iter().any(|file| file == "package.json") {
        return (
            "implicit".to_string(),
            "No tsconfig/jsconfig found; TypeScript server will use inferred project indexing from opened JS/TS files.".to_string(),
        );
    }
    if has_files_with_extensions(workspace_root, &["ts", "tsx", "js", "jsx"]) {
        return (
            "implicit".to_string(),
            "TypeScript/JavaScript files found without package.json or tsconfig/jsconfig; indexing is limited to inferred projects.".to_string(),
        );
    }
    (
        "empty".to_string(),
        "No TypeScript/JavaScript project files detected in the workspace.".to_string(),
    )
}

fn infer_python_indexing_state(workspace_root: &Path, config_files: &[String]) -> (String, String) {
    if config_files.iter().any(|file| file == "pyproject.toml") {
        return (
            "configured".to_string(),
            "Workspace has pyproject.toml; Pyright can use project configuration and environment metadata.".to_string(),
        );
    }
    if config_files.iter().any(|file| {
        matches!(
            file.as_str(),
            "setup.py" | "setup.cfg" | "requirements.txt" | "Pipfile" | "poetry.lock"
        )
    }) {
        return (
            "implicit".to_string(),
            "Python project markers found; Pyright will index opened files and infer imports from workspace/environment.".to_string(),
        );
    }
    if has_files_with_extensions(workspace_root, &["py"]) {
        return (
            "adhoc".to_string(),
            "Python files found without project config; indexing is limited to opened files and inferred paths.".to_string(),
        );
    }
    (
        "empty".to_string(),
        "No Python project files detected in the workspace.".to_string(),
    )
}

fn infer_rust_indexing_state(workspace_root: &Path, config_files: &[String]) -> (String, String) {
    if config_files.iter().any(|file| file == "Cargo.toml") {
        return (
            "crate".to_string(),
            "Workspace has Cargo.toml; rust-analyzer can index the crate graph.".to_string(),
        );
    }
    if config_files.iter().any(|file| file == "rust-project.json") {
        return (
            "configured".to_string(),
            "Workspace has rust-project.json; rust-analyzer can use explicit project layout."
                .to_string(),
        );
    }
    if has_files_with_extensions(workspace_root, &["rs"]) {
        return (
            "standalone".to_string(),
            "Rust files found without Cargo.toml; rust-analyzer will use limited standalone-file analysis.".to_string(),
        );
    }
    (
        "empty".to_string(),
        "No Rust project files detected in the workspace.".to_string(),
    )
}

fn infer_go_indexing_state(workspace_root: &Path, config_files: &[String]) -> (String, String) {
    if config_files.iter().any(|file| file == "go.work") {
        return (
            "workspace".to_string(),
            "Workspace has go.work; gopls can index multiple Go modules.".to_string(),
        );
    }
    if config_files.iter().any(|file| file == "go.mod") {
        return (
            "module".to_string(),
            "Workspace has go.mod; gopls can index this Go module.".to_string(),
        );
    }
    if has_files_with_extensions(workspace_root, &["go"]) {
        return (
            "gopath".to_string(),
            "Go files found without go.mod/go.work; gopls will use GOPATH or ad-hoc package indexing.".to_string(),
        );
    }
    (
        "empty".to_string(),
        "No Go module or Go files detected in the workspace.".to_string(),
    )
}

fn has_files_with_extensions(workspace_root: &Path, extensions: &[&str]) -> bool {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return false;
    };
    entries.flatten().take(200).any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(ext))
            })
            .unwrap_or(false)
    })
}

fn platform_server_binary_name(spec: &LspSpec) -> &'static str {
    if cfg!(windows) {
        spec.windows_binary_names[0]
    } else {
        spec.binary_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_uri_round_trips_spaces_and_symbols() {
        let path = if cfg!(windows) {
            PathBuf::from(r"D:\work\agent ide\src\[demo].ts")
        } else {
            PathBuf::from("/tmp/agent ide/src/[demo].ts")
        };
        let uri = path_to_uri(&path);

        assert!(uri.contains("agent%20ide"));
        assert!(uri.contains("%5Bdemo%5D.ts"));
        assert_eq!(uri_to_path(&uri).map(PathBuf::from), Some(path));
    }

    #[test]
    fn lsp_uri_decode_rejects_invalid_percent_encoding() {
        assert_eq!(uri_to_path("file:///D:/work/%ZZ/demo.ts"), None);
    }

    #[test]
    fn lsp_uri_strips_windows_verbatim_prefix() {
        if !cfg!(windows) {
            return;
        }

        let uri = path_to_uri(Path::new(r"\\?\D:\work\agent-ide\src\main.ts"));
        assert_eq!(uri, "file:///D:/work/agent-ide/src/main.ts");
    }

    #[test]
    fn lsp_indexing_state_detects_configured_workspace() {
        let spec = lsp_spec("typescript").unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("lsp-indexing-configured");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();

        let configs = detect_lsp_workspace_config_files(&root, &spec);
        let (status, message) = infer_indexing_state(&root, &configs, &spec);

        assert_eq!(configs, vec!["tsconfig.json".to_string()]);
        assert_eq!(status, "configured");
        assert!(message.contains("tsconfig/jsconfig"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lsp_indexing_state_detects_implicit_workspace() {
        let spec = lsp_spec("typescript").unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("lsp-indexing-implicit");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();

        let configs = detect_lsp_workspace_config_files(&root, &spec);
        let (status, message) = infer_indexing_state(&root, &configs, &spec);

        assert_eq!(configs, vec!["package.json".to_string()]);
        assert_eq!(status, "implicit");
        assert!(message.contains("inferred"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lsp_indexing_state_detects_go_module() {
        let spec = lsp_spec("go").unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("lsp-indexing-go-module");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("go.mod"), "module demo\n").unwrap();

        let configs = detect_lsp_workspace_config_files(&root, &spec);
        let (status, message) = infer_indexing_state(&root, &configs, &spec);

        assert_eq!(configs, vec!["go.mod".to_string()]);
        assert_eq!(status, "module");
        assert!(message.contains("go.mod"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lsp_indexing_state_detects_python_project() {
        let spec = lsp_spec("python").unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("lsp-indexing-python-project");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"demo\"\n").unwrap();

        let configs = detect_lsp_workspace_config_files(&root, &spec);
        let (status, message) = infer_indexing_state(&root, &configs, &spec);

        assert_eq!(configs, vec!["pyproject.toml".to_string()]);
        assert_eq!(status, "configured");
        assert!(message.contains("pyproject.toml"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lsp_indexing_state_detects_rust_crate() {
        let spec = lsp_spec("rust").unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("lsp-indexing-rust-crate");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

        let configs = detect_lsp_workspace_config_files(&root, &spec);
        let (status, message) = infer_indexing_state(&root, &configs, &spec);

        assert_eq!(configs, vec!["Cargo.toml".to_string()]);
        assert_eq!(status, "crate");
        assert!(message.contains("Cargo.toml"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
