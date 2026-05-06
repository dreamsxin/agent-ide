use crate::agent::state_machine::{AgentState, AgentMode, TaskStep, FileDiff, ApplyDiffError, ApplyDiffsResult};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::agent::multi_agent::{AgentRole, PipelineStage, default_pipeline};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::context::{AgentContext, ContextCompressionMode};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::Mutex;
use tauri::{AppHandle, State};
use tauri::Emitter;
use crate::services::workspace;

/// Agent 全局状态（使用 tokio::sync::Mutex 以支持 async 上下文中持有锁）

/// 获取 Agent IDE 配置目录（~/.agent-ide）
/// 保存 LLM 配置到磁盘
fn save_llm_config_to_disk(config: &LlmConfig, context_compression: &ContextCompressionMode) {
    let dir = workspace::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.json");
    if let Ok(json) = serde_json::to_string_pretty(&serde_json::json!({
        "endpoint": config.endpoint,
        "api_key": config.api_key,
        "model": config.model,
        "context_compression": context_compression.to_string(),
    })) {
        let _ = std::fs::write(&path, json);
    }
}

/// 从磁盘加载 LLM 配置
fn load_llm_config_from_disk() -> Option<(LlmConfig, ContextCompressionMode)> {
    let path = workspace::config_dir().join("config.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let context_compression = parsed
        .get("context_compression")
        .and_then(|v| v.as_str())
        .and_then(|v| ContextCompressionMode::from_str(v).ok())
        .unwrap_or_default();
    Some((LlmConfig {
        endpoint: parsed.get("endpoint")?.as_str()?.to_string(),
        api_key: parsed.get("api_key")?.as_str()?.to_string(),
        model: parsed.get("model")?.as_str()?.to_string(),
    }, context_compression))
}

pub struct AgentGlobalState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub llm_config: Arc<std::sync::Mutex<Option<LlmConfig>>>,
    pub llm_client: Arc<std::sync::Mutex<Option<LlmClient>>>,
    pub active_role: Arc<std::sync::Mutex<AgentRole>>,
    pub pipeline_stages: Arc<std::sync::Mutex<Vec<PipelineStage>>>,
    pub context_compression: Arc<std::sync::Mutex<ContextCompressionMode>>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl AgentGlobalState {
    pub fn new() -> Self {
        // 优先从磁盘加载上次配置，否则用环境变量 / 默认值
        let (config, context_compression) = load_llm_config_from_disk().unwrap_or_else(|| {
            let endpoint = std::env::var("LLM_ENDPOINT")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());
            let mode = std::env::var("AGENT_CONTEXT_COMPRESSION")
                .ok()
                .and_then(|v| ContextCompressionMode::from_str(&v).ok())
                .unwrap_or_default();
            (LlmConfig { endpoint, api_key, model }, mode)
        });

        let client = LlmClient::new(config.clone());

        Self {
            orchestrator: Arc::new(Mutex::new(AgentOrchestrator::new())),
            llm_config: Arc::new(std::sync::Mutex::new(Some(config))),
            llm_client: Arc::new(std::sync::Mutex::new(Some(client))),
            active_role: Arc::new(std::sync::Mutex::new(AgentRole::Coder)),
            pipeline_stages: Arc::new(std::sync::Mutex::new(default_pipeline())),
            context_compression: Arc::new(std::sync::Mutex::new(context_compression)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 获取 LLM 客户端引用
    pub fn get_llm_client(&self) -> Result<LlmClient, String> {
        self.llm_client
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or_else(|| "LLM client not initialized".to_string())
    }
}

/// Agent 状态响应 DTO
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
    pub context_files: Vec<String>,
}

/// 发送 Prompt 请求 DTO
#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
}

/// 获取 Agent 当前状态
#[tauri::command]
pub async fn get_agent_state(agent_state: State<'_, AgentGlobalState>) -> Result<AgentStatus, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(AgentStatus {
        state: orch.state_mgr.state.to_string(),
        mode: orch.mode.to_string(),
        context_files: Vec::new(),
    })
}

/// 发送 Prompt 到 Agent
#[tauri::command]
pub async fn send_agent_prompt(
    request: SendPromptRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client()?;

    let context = AgentContext {
        active_file: request.active_file,
        active_file_content: request.active_file_content,
        selection: request.selection,
        open_files: request.context_files,
        project_path: workspace::workspace_root_string(),
    };

    // 使用 tokio::sync::Mutex 可以安全地在 async 中持有锁
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    agent_state.cancel_flag.store(false, Ordering::SeqCst);
    let cancel_flag = agent_state.cancel_flag.clone();
    let mut orch = agent_state.orchestrator.lock().await;
    match orch.run(request.prompt, context, compression, cancel_flag, &llm, app_handle).await {
        Ok(()) => {}
        Err(err) if is_cancelled_error(&err) => return Ok("Agent task cancelled".to_string()),
        Err(err) => return Err(err),
    }

    Ok("Agent task completed".to_string())
}

/// 停止 Agent 当前任务
#[tauri::command]
pub async fn stop_agent(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    agent_state.cancel_flag.store(true, Ordering::SeqCst);
    let mut orch = agent_state.orchestrator.lock().await;
    orch.state_mgr.set(AgentState::Idle);
    orch.steps.clear();
    orch.diffs.clear();
    Ok("Agent stopped".to_string())
}

/// 设置 Agent 模式
#[tauri::command]
pub async fn set_agent_mode(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.mode = match mode.as_str() {
        "suggest" => AgentMode::Suggest,
        "edit" => AgentMode::Edit,
        "auto" => AgentMode::Auto,
        _ => return Err(format!("Invalid mode: {}", mode)),
    };
    Ok(())
}

/// 应用所有 pending diffs —— 实际写入文件系统
#[tauri::command]
#[allow(unreachable_code)]
pub async fn apply_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let result = apply_pending_diffs(&orch.diffs);
    let applied = result.applied.clone();
    let failed = result.failed.clone();

    for diff in &mut orch.diffs {
        if diff.status != "pending" {
            continue;
        }
        if applied.iter().any(|item| item.id == diff.id) {
            diff.status = "applied".to_string();
        } else if failed.iter().any(|item| item.diff_id == diff.id) {
            diff.status = "failed".to_string();
        }
    }

    if failed.is_empty() {
        orch.state_mgr
            .transition(&crate::agent::state_machine::AgentEvent::UserApply);
    } else {
        orch.state_mgr.set(AgentState::WaitingUser);
    }
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    return Ok(result);
    let diffs = orch.diffs.clone();

    let mut applied: Vec<FileDiff> = Vec::new();
    let mut failed: Vec<ApplyDiffError> = Vec::new();

    for diff in &diffs {
        if diff.status != "pending" {
            continue;
        }

        // 拼接项目路径 + 文件路径
        let file_path = match workspace::resolve_for_write(&diff.file) {
            Ok(path) => path,
            Err(err) => {
                failed.push(ApplyDiffError {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    message: err,
                });
                continue;
            }
        };
        // 确保父目录存在
        let result = if let Some(parent) = file_path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                failed.push(ApplyDiffError {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    message: format!("Create dir failed: {}", err),
                });
                continue;
            }
            apply_diff_to_path(&file_path, diff)
        } else {
            apply_diff_to_path(&file_path, diff)
        };

        match result {
            Ok(true) => {
                applied.push(diff.clone());
            }
            Ok(false) => {}
            Err(message) => {
                failed.push(ApplyDiffError {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    message,
                });
            }
        }
        /*
        let mut written = false;
        let mut diff_error: Option<String> = None;
        for hunk in &diff.hunks {
            if hunk.original.is_empty() && !hunk.updated.is_empty() {
                // 新文件：直接写入 updated 内容
                if let Err(err) = fs::write(&file_path, &hunk.updated) {
                    diff_error = Some(format!("Write new file {}: {}", file_path.display(), err));
                    break;
                }
                written = true;
            } else if !hunk.original.is_empty() {
                // 编辑已有文件：读取 → 替换 → 写回
                let existing = match fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(_) => {
                        diff_error = Some(format!("File not found: {}", file_path.display()));
                        break;
                    }
                };

                // 在文件内容中查找并替换 original → updated
                if let Some(replaced) = replace_first(&existing, &hunk.original, &hunk.updated) {
                    if let Err(err) = fs::write(&file_path, &replaced) {
                        diff_error = Some(format!("Write {}: {}", file_path.display(), err));
                        break;
                    }
                    written = true;
                } else {
                    diff_error = Some(format!(
                        "Could not find original content in {}: {}",
                        file_path.display(),
                        hunk.original[..hunk.original.len().min(200)].replace('\n', "\\n")
                    ));
                    break;
                }
            }
        }

        if let Some(message) = diff_error {
            failed.push(ApplyDiffError {
                diff_id: diff.id.clone(),
                file: diff.file.clone(),
                message,
            });
        } else if written {
            applied.push(diff.clone());
        }
        */
    }

    // 标记 applied diffs
    for diff in &mut orch.diffs {
        if diff.status != "pending" {
            continue;
        }
        if applied.iter().any(|item| item.id == diff.id) {
            diff.status = "applied".to_string();
        } else if failed.iter().any(|item| item.diff_id == diff.id) {
            diff.status = "failed".to_string();
        }
    }

    if failed.is_empty() {
        orch.state_mgr
            .transition(&crate::agent::state_machine::AgentEvent::UserApply);
    } else {
        orch.state_mgr.set(AgentState::WaitingUser);
    }
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    Ok(ApplyDiffsResult { applied, failed })
}

fn apply_diff_to_path(file_path: &std::path::Path, diff: &FileDiff) -> Result<bool, String> {
    use std::fs;

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create dir failed: {}", e))?;
    }

    let mut written = false;
    for hunk in &diff.hunks {
        if hunk.original.is_empty() && !hunk.updated.is_empty() {
            fs::write(file_path, &hunk.updated)
                .map_err(|e| format!("Write new file {}: {}", file_path.display(), e))?;
            written = true;
        } else if !hunk.original.is_empty() {
            let existing = fs::read_to_string(file_path)
                .map_err(|_| format!("File not found: {}", file_path.display()))?;

            if let Some(replaced) = replace_first(&existing, &hunk.original, &hunk.updated) {
                fs::write(file_path, &replaced)
                    .map_err(|e| format!("Write {}: {}", file_path.display(), e))?;
                written = true;
            } else {
                return Err(format!(
                    "Could not find original content in {}: {}",
                    file_path.display(),
                    hunk.original[..hunk.original.len().min(200)].replace('\n', "\\n")
                ));
            }
        }
    }

    Ok(written)
}

#[cfg_attr(not(test), allow(dead_code))]
fn apply_pending_diffs(diffs: &[FileDiff]) -> ApplyDiffsResult {
    let mut applied: Vec<FileDiff> = Vec::new();
    let mut failed: Vec<ApplyDiffError> = Vec::new();

    for diff in diffs {
        if diff.status != "pending" {
            continue;
        }

        let file_path = match workspace::resolve_for_write(&diff.file) {
            Ok(path) => path,
            Err(err) => {
                failed.push(ApplyDiffError {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    message: err,
                });
                continue;
            }
        };

        match apply_diff_to_path(&file_path, diff) {
            Ok(true) => applied.push(diff.clone()),
            Ok(false) => {}
            Err(message) => failed.push(ApplyDiffError {
                diff_id: diff.id.clone(),
                file: diff.file.clone(),
                message,
            }),
        }
    }

    ApplyDiffsResult { applied, failed }
}

/// 在文本中查找并替换首次出现的 original → updated
fn replace_first(text: &str, original: &str, updated: &str) -> Option<String> {
    // 精确匹配
    if let Some(pos) = text.find(original) {
        let mut result = String::with_capacity(text.len() + updated.len());
        result.push_str(&text[..pos]);
        result.push_str(updated);
        result.push_str(&text[pos + original.len()..]);
        return Some(result);
    }
    // 模糊匹配：trim 后比较
    let orig_trim = original.trim();
    if orig_trim != original {
        if let Some(pos) = text.find(orig_trim) {
            let mut result = String::with_capacity(text.len() + updated.len());
            result.push_str(&text[..pos]);
            result.push_str(updated.trim());
            result.push_str(&text[pos + orig_trim.len()..]);
            return Some(result);
        }
    }
    None
}

fn is_cancelled_error(err: &str) -> bool {
    err == "Agent task cancelled"
}

/// 拒绝所有 pending diffs
#[tauri::command]
pub async fn reject_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.reject_diffs();

    orch.state_mgr
        .transition(&crate::agent::state_machine::AgentEvent::UserReject);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    let rejected: Vec<FileDiff> = orch
        .diffs
        .iter()
        .filter(|d| d.status == "rejected")
        .cloned()
        .collect();

    Ok(rejected)
}

/// 获取当前 steps
#[tauri::command]
pub async fn get_agent_steps(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.steps.clone())
}

/// 获取当前 diffs
#[tauri::command]
pub async fn get_agent_diffs(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.diffs.clone())
}

/// 更新 LLM 配置
#[tauri::command]
pub async fn update_llm_config(
    endpoint: String,
    api_key: String,
    model: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let config = LlmConfig {
        endpoint,
        api_key,
        model,
    };

    let client = LlmClient::new(config.clone());

    {
        let mut cfg = agent_state.llm_config.lock().map_err(|e| e.to_string())?;
        *cfg = Some(config.clone());
    }
    {
        let mut cli = agent_state.llm_client.lock().map_err(|e| e.to_string())?;
        *cli = Some(client);
    }

    // 持久化到 ~/.agent-ide/config.json
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    save_llm_config_to_disk(&config, &compression);

    Ok(())
}

/// 获取 LLM 配置（api_key 脱敏）
#[derive(Debug, Serialize)]
pub struct LlmConfigResponse {
    pub endpoint: String,
    pub api_key_masked: String,
    pub model: String,
    pub context_compression: String,
}

#[tauri::command]
pub async fn get_llm_config(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmConfigResponse, String> {
    let cfg = agent_state.llm_config.lock().map_err(|e| e.to_string())?;
    match &*cfg {
        Some(c) => {
            let masked = if c.api_key.len() > 8 {
                format!("{}****{}", &c.api_key[..4], &c.api_key[c.api_key.len()-4..])
            } else {
                "****".to_string()
            };
            Ok(LlmConfigResponse {
                endpoint: c.endpoint.clone(),
                api_key_masked: masked,
                model: c.model.clone(),
                context_compression: agent_state
                    .context_compression
                    .lock()
                    .map_err(|e| e.to_string())?
                    .to_string(),
            })
        }
        None => Err("LLM config not set".to_string()),
    }
}

#[tauri::command]
pub async fn set_context_compression(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = ContextCompressionMode::from_str(&mode)?;
    {
        let mut current = agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())?;
        *current = parsed.clone();
    }
    if let Some(config) = agent_state
        .llm_config
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
    {
        save_llm_config_to_disk(&config, &parsed);
    }
    Ok(parsed.to_string())
}

/// 保存工作目录路径到磁盘
#[tauri::command]
pub fn save_workspace_path(path: String) -> Result<(), String> {
    let resolved = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e))?;
    if !resolved.is_dir() {
        return Err(format!("Workspace is not a directory: {}", path));
    }
    workspace::save_workspace_path(&resolved.to_string_lossy())
}

/// 从磁盘加载上次保存的工作目录
#[tauri::command]
pub fn get_workspace_path() -> Result<Option<String>, String> {
    workspace::load_workspace_path()
}

/// 测试 LLM 连通性：发送简单请求验证 API 可用
#[tauri::command]
pub async fn test_llm_connection(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client()?;

    let messages = vec![
        crate::services::llm_client::ChatMessage {
            role: "user".to_string(),
            content: "Hi".to_string(),
        },
    ];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
    let handle = tokio::spawn(async move {
        let mut full = String::new();
        while let Some(tok) = rx.recv().await {
            full.push_str(&tok);
        }
        full
    });

    match llm.stream_chat(messages, agent_state.cancel_flag.clone(), tx).await {
        Ok(response) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let full = handle.await.unwrap_or(response);
            Ok(format!("OK — {}", &full[..full.len().min(120)]))
        }
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}


/// 设置当前 Agent 角色
#[tauri::command]
pub async fn set_active_role(
    role: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = match role.as_str() {
        "architect" => AgentRole::Architect,
        "coder" => AgentRole::Coder,
        "tester" => AgentRole::Tester,
        "reviewer" => AgentRole::Reviewer,
        _ => return Err(format!("Invalid role: {}", role)),
    };
    let mut active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    *active = parsed;
    Ok(parsed.to_string().to_string())
}

/// 获取当前 Agent 角色
#[tauri::command]
pub async fn get_active_role(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    Ok(active.to_string().to_string())
}

/// 获取当前流水线
#[tauri::command]
pub async fn get_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let stages = agent_state.pipeline_stages.lock().map_err(|e| e.to_string())?;
    Ok(stages.clone())
}

/// 更新流水线
#[tauri::command]
pub async fn update_pipeline(
    stages: Vec<PipelineStage>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut pipe = agent_state.pipeline_stages.lock().map_err(|e| e.to_string())?;
    *pipe = stages;
    Ok(())
}

/// 重置流水线为默认
#[tauri::command]
pub async fn reset_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let mut pipe = agent_state.pipeline_stages.lock().map_err(|e| e.to_string())?;
    *pipe = default_pipeline();
    Ok(pipe.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state_machine::DiffHunk;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-ide-apply-diff-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = temp_dir();
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write_file(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(
                self.root
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| self.root.clone()),
            );
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    fn make_diff(file: &str, original: &str, updated: &str) -> FileDiff {
        FileDiff {
            id: Uuid::new_v4().to_string(),
            file: file.to_string(),
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: original.lines().count().max(1) as u32,
                new_start: 1,
                new_lines: updated.lines().count().max(1) as u32,
                content: String::new(),
                original: original.to_string(),
                updated: updated.to_string(),
            }],
            status: "pending".to_string(),
        }
    }

    #[test]
    fn apply_diff_to_path_creates_new_file() {
        let dir = temp_dir();
        let path = dir.join("new-file.ts");
        let diff = make_diff("new-file.ts", "", "export const created = true;\n");

        let written = apply_diff_to_path(&path, &diff).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(written);
        assert_eq!(content, "export const created = true;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_updates_existing_file() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\nconsole.log(value);\n").unwrap();
        let diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");

        let written = apply_diff_to_path(&path, &diff).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(written);
        assert!(content.contains("const value = 2;"));
        assert!(!content.contains("const value = 1;"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_reports_missing_original() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\n").unwrap();
        let diff = make_diff("edit.ts", "const value = 9;", "const value = 2;");

        let err = apply_diff_to_path(&path, &diff).unwrap_err();

        assert!(err.contains("Could not find original content"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_pending_diffs_reports_partial_success() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/ok.ts", "const value = 1;\n");
        env.write_file("src/fail.ts", "const other = 1;\n");

        let ok_diff = make_diff("src/ok.ts", "const value = 1;", "const value = 2;");
        let fail_diff = make_diff("src/fail.ts", "const missing = 1;", "const value = 2;");

        let result = apply_pending_diffs(&[ok_diff.clone(), fail_diff.clone()]);

        assert_eq!(result.applied.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.applied[0].id, ok_diff.id);
        assert_eq!(result.failed[0].diff_id, fail_diff.id);
        assert!(result.failed[0].message.contains("Could not find original content"));
        assert_eq!(
            std::fs::read_to_string(env.root.join("src/ok.ts")).unwrap(),
            "const value = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("src/fail.ts")).unwrap(),
            "const other = 1;\n"
        );
    }
}
