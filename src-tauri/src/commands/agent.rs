use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::multi_agent::{default_pipeline, AgentRole, PipelineStage};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::agent::state_machine::{AgentMode, AgentState, ApplyDiffsResult, FileDiff, TaskStep};
use crate::services::context::{AgentContext, ContextCompressionMode};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Emitter;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

/// Global Agent state. Uses tokio::sync::Mutex for async orchestration.

/// Save LLM configuration to disk.
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

/// Load LLM configuration from disk.
fn load_llm_config_from_disk() -> Option<(LlmConfig, ContextCompressionMode)> {
    let path = workspace::config_dir().join("config.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let context_compression = parsed
        .get("context_compression")
        .and_then(|v| v.as_str())
        .and_then(|v| ContextCompressionMode::from_str(v).ok())
        .unwrap_or_default();
    Some((
        LlmConfig {
            endpoint: parsed.get("endpoint")?.as_str()?.to_string(),
            api_key: parsed.get("api_key")?.as_str()?.to_string(),
            model: parsed.get("model")?.as_str()?.to_string(),
        },
        context_compression,
    ))
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
        let (config, context_compression) = load_llm_config_from_disk().unwrap_or_else(|| {
            let endpoint = std::env::var("LLM_ENDPOINT")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());
            let mode = std::env::var("AGENT_CONTEXT_COMPRESSION")
                .ok()
                .and_then(|v| ContextCompressionMode::from_str(&v).ok())
                .unwrap_or_default();
            (
                LlmConfig {
                    endpoint,
                    api_key,
                    model,
                },
                mode,
            )
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

    /// Get a cloned LLM client.
    pub fn get_llm_client(&self) -> Result<LlmClient, String> {
        self.llm_client
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or_else(|| "LLM client not initialized".to_string())
    }
}

/// Agent status response DTO.
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
    pub context_files: Vec<String>,
}

/// Send-prompt request DTO.
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

/// Get the current Agent state.
#[tauri::command]
pub async fn get_agent_state(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<AgentStatus, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(AgentStatus {
        state: orch.state_mgr.state.to_string(),
        mode: orch.mode.to_string(),
        context_files: Vec::new(),
    })
}

/// Send a prompt to the Agent.
#[tauri::command]
pub async fn send_agent_prompt(
    request: SendPromptRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client()?;

    let mut context = AgentContext {
        active_file: request.active_file,
        active_file_content: request.active_file_content,
        selection: request.selection,
        open_files: request.context_files,
        project_path: workspace::workspace_root_string(),
        git_diff: None,
        project_tree: None,
    };
    context.enrich_from_workspace();

    // The async mutex can be held safely while the orchestrator runs.
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let pipeline = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    agent_state.cancel_flag.store(false, Ordering::SeqCst);
    let cancel_flag = agent_state.cancel_flag.clone();
    let mut orch = agent_state.orchestrator.lock().await;
    match orch
        .run(
            request.prompt,
            context,
            compression,
            pipeline,
            cancel_flag,
            &llm,
            app_handle.clone(),
        )
        .await
    {
        Ok(()) => {}
        Err(err) if is_cancelled_error(&err) => {
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
            );
            return Ok("Agent task cancelled".to_string());
        }
        Err(err) => return Err(err),
    }

    Ok("Agent task completed".to_string())
}

/// Stop the current Agent task.
#[tauri::command]
pub async fn stop_agent(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    agent_state.cancel_flag.store(true, Ordering::SeqCst);
    let mut orch = agent_state.orchestrator.lock().await;
    orch.state_mgr.set(AgentState::Idle);
    orch.steps.clear();
    orch.diffs.clear();
    Ok("Agent stopped".to_string())
}

/// Set the Agent mode.
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

/// Apply all pending diffs to the filesystem.
#[tauri::command]
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

    Ok(result)
}

/// Apply one pending diff to the filesystem.
#[tauri::command]
pub async fn apply_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch
        .diffs
        .iter()
        .find(|item| item.id == diff_id)
        .cloned()
    else {
        return Err(format!("Diff not found: {}", diff_id));
    };

    if diff.status != "pending" {
        return Err(format!(
            "Diff {} is not pending; current status is {}",
            diff_id, diff.status
        ));
    }

    let result = apply_pending_diffs(&[diff]);
    let failed = result.failed.clone();

    for item in &mut orch.diffs {
        if item.id != diff_id {
            continue;
        }
        if result.applied.iter().any(|applied| applied.id == item.id) {
            item.status = "applied".to_string();
        } else if failed.iter().any(|failure| failure.diff_id == item.id) {
            item.status = "failed".to_string();
        }
    }

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    Ok(result)
}

fn is_cancelled_error(err: &str) -> bool {
    err == "Agent task cancelled"
}

/// Reject all pending diffs.
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

/// Reject one pending diff.
#[tauri::command]
pub async fn reject_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<FileDiff, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch.diffs.iter_mut().find(|item| item.id == diff_id) else {
        return Err(format!("Diff not found: {}", diff_id));
    };

    if diff.status != "pending" {
        return Err(format!(
            "Diff {} is not pending; current status is {}",
            diff_id, diff.status
        ));
    }

    diff.status = "rejected".to_string();
    let rejected = diff.clone();

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    Ok(rejected)
}

fn set_review_state_after_single_diff(orch: &mut AgentOrchestrator) {
    if orch
        .diffs
        .iter()
        .any(|diff| diff.status == "pending" || diff.status == "failed")
    {
        orch.state_mgr.set(AgentState::WaitingUser);
    } else {
        orch.state_mgr.set(AgentState::Done);
    }
}

/// Get the current steps.
#[tauri::command]
pub async fn get_agent_steps(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.steps.clone())
}

/// Get the current diffs.
#[tauri::command]
pub async fn get_agent_diffs(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.diffs.clone())
}

/// Update LLM configuration.
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

    // 鎸佷箙鍖栧埌 ~/.agent-ide/config.json
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    save_llm_config_to_disk(&config, &compression);

    Ok(())
}

/// Get LLM configuration with the API key masked.
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
                format!(
                    "{}****{}",
                    &c.api_key[..4],
                    &c.api_key[c.api_key.len() - 4..]
                )
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

/// Save the workspace path to disk.
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

/// Load the last saved workspace path from disk.
#[tauri::command]
pub fn get_workspace_path() -> Result<Option<String>, String> {
    workspace::load_workspace_path()
}

/// Test LLM connectivity with a small request.
#[tauri::command]
pub async fn test_llm_connection(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client()?;

    let messages = vec![crate::services::llm_client::ChatMessage {
        role: "user".to_string(),
        content: "Hi".to_string(),
    }];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
    let handle = tokio::spawn(async move {
        let mut full = String::new();
        while let Some(tok) = rx.recv().await {
            full.push_str(&tok);
        }
        full
    });

    match llm
        .stream_chat(messages, agent_state.cancel_flag.clone(), tx)
        .await
    {
        Ok(response) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let full = handle.await.unwrap_or(response);
            Ok(format!("OK - {}", &full[..full.len().min(120)]))
        }
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

/// Set the current Agent role.
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

/// Get the current Agent role.
#[tauri::command]
pub async fn get_active_role(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    let active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    Ok(active.to_string().to_string())
}

/// Get the current pipeline.
#[tauri::command]
pub async fn get_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let stages = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(stages.clone())
}

/// Update the pipeline.
#[tauri::command]
pub async fn update_pipeline(
    stages: Vec<PipelineStage>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = stages;
    Ok(())
}

/// Reset the pipeline to defaults.
#[tauri::command]
pub async fn reset_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = default_pipeline();
    Ok(pipe.clone())
}
