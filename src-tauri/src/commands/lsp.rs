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

#[derive(Debug, Clone, Serialize)]
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
) -> Result<(), String> {
    let root = match workspace_path {
        Some(path) if !path.trim().is_empty() => workspace::resolve_existing(&path)?,
        _ => workspace::workspace_root()?,
    };
    let root = workspace::shell_compatible_path(root);
    {
        let server = manager.inner.server.lock().map_err(|e| e.to_string())?;
        if server
            .as_ref()
            .map(|server| server.workspace_root == root)
            .unwrap_or(false)
        {
            return Ok(());
        }
    }

    let executable = find_typescript_language_server(&root).ok_or_else(|| {
        "Start TypeScript LSP failed: typescript-language-server was not found. Install globally with: npm install -g typescript typescript-language-server, or add it to this workspace devDependencies.".to_string()
    })?;
    let mut child = Command::new(&executable)
        .arg("--stdio")
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Start TypeScript LSP failed: {}. Executable: {}",
                e,
                executable.display()
            )
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "TypeScript LSP stdin is unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "TypeScript LSP stdout is unavailable".to_string())?;

    {
        let mut server = manager.inner.server.lock().map_err(|e| e.to_string())?;
        *server = Some(LspServer {
            child,
            stdin,
            pending: HashMap::new(),
            workspace_root: root.clone(),
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
        return Err(format!("TypeScript LSP initialize failed: {}", initialize));
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
) -> Result<Vec<LspCodeAction>, String> {
    let file = workspace::resolve_existing(&file)?;
    let response = request(
        &manager,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": path_to_uri(&file) },
            "range": range,
            "context": { "diagnostics": [] }
        }),
    )?;
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    Ok(parse_code_actions(result))
}

fn request(manager: &LspManager, method: &str, params: Value) -> Result<Value, String> {
    let id = manager.inner.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    {
        let mut guard = manager.inner.server.lock().map_err(|e| e.to_string())?;
        let server = guard
            .as_mut()
            .ok_or_else(|| "TypeScript LSP is not initialized".to_string())?;
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
        .ok_or_else(|| "TypeScript LSP is not initialized".to_string())?;
    write_message(
        &mut server.stdin,
        &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    )
}

fn write_message(stdin: &mut ChildStdin, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.as_bytes().len(), body)
        .map_err(|e| format!("Write LSP message: {}", e))?;
    stdin.flush().map_err(|e| format!("Flush LSP: {}", e))
}

fn read_lsp_output(app: AppHandle, state: Arc<LspInner>, stdout: std::process::ChildStdout) {
    let mut reader = BufReader::new(stdout);
    loop {
        let message = match read_message(&mut reader) {
            Ok(Some(message)) => message,
            Ok(None) => {
                let _ = app.emit("lsp-status", LspStatusEvent {
                    status: "error".to_string(),
                    message: "TypeScript LSP stdout closed.".to_string(),
                });
                break;
            }
            Err(error) => {
                let _ = app.emit("lsp-status", LspStatusEvent {
                    status: "error".to_string(),
                    message: error,
                });
                break;
            }
        };
        let method = message.get("method").and_then(Value::as_str);
        if let Some(id) = message.get("id").and_then(Value::as_u64).filter(|_| method.is_none()) {
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
                let _ = app.emit("lsp-diagnostics", diagnostics);
            }
        }
    }
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
                let section = item.get("section").and_then(Value::as_str).unwrap_or_default();
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
                    _ => Value::Null,
                }
            })
            .collect(),
    )
}

fn read_message(reader: &mut BufReader<std::process::ChildStdout>) -> Result<Option<Value>, String> {
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
                        source: item.get("source").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    LspDiagnosticsEvent { file, diagnostics }
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
                        detail: item.get("detail").and_then(Value::as_str).map(str::to_string),
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
                        sort_text: item.get("sortText").and_then(Value::as_str).map(str::to_string),
                        filter_text: item.get("filterText").and_then(Value::as_str).map(str::to_string),
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
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("value").and_then(Value::as_str).map(str::to_string))
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
    let text = path.to_string_lossy().replace('\\', "/");
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
    Some(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

impl Drop for LspServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn find_typescript_language_server(workspace_root: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(workspace_root.join("node_modules").join(".bin").join(server_binary_name()));

    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            candidates.push(PathBuf::from(appdata).join("npm").join("typescript-language-server.cmd"));
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path) {
            candidates.push(entry.join(server_binary_name()));
            if cfg!(windows) {
                candidates.push(entry.join("typescript-language-server.cmd"));
                candidates.push(entry.join("typescript-language-server.exe"));
            }
        }
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn server_binary_name() -> &'static str {
    if cfg!(windows) {
        "typescript-language-server.cmd"
    } else {
        "typescript-language-server"
    }
}
