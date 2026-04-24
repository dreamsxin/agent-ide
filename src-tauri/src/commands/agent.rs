use crate::agent::state_machine::{AgentState, AgentMode, TaskStep, FileDiff};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::context::AgentContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{AppHandle, State};
use tauri::Emitter;

/// Agent 全局状态（使用 tokio::sync::Mutex 以支持 async 上下文中持有锁）
pub struct AgentGlobalState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub llm_config: Arc<std::sync::Mutex<Option<LlmConfig>>>,
    pub llm_client: Arc<std::sync::Mutex<Option<LlmClient>>>,
}

impl AgentGlobalState {
    pub fn new() -> Self {
        let endpoint = std::env::var("LLM_ENDPOINT")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());

        let config = LlmConfig {
            endpoint,
            api_key,
            model,
        };

        let client = LlmClient::new(config.clone());

        Self {
            orchestrator: Arc::new(Mutex::new(AgentOrchestrator::new())),
            llm_config: Arc::new(std::sync::Mutex::new(Some(config))),
            llm_client: Arc::new(std::sync::Mutex::new(Some(client))),
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
        project_path: std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    };

    // 使用 tokio::sync::Mutex 可以安全地在 async 中持有锁
    let mut orch = agent_state.orchestrator.lock().await;
    orch.run(request.prompt, context, &llm, app_handle).await?;

    Ok("Agent task completed".to_string())
}

/// 停止 Agent 当前任务
#[tauri::command]
pub async fn stop_agent(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
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

/// 应用所有 pending diffs
#[tauri::command]
pub async fn apply_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.apply_diffs();

    orch.state_mgr
        .transition(&crate::agent::state_machine::AgentEvent::UserApply);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    let applied: Vec<FileDiff> = orch
        .diffs
        .iter()
        .filter(|d| d.status == "applied")
        .cloned()
        .collect();

    Ok(applied)
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
        *cfg = Some(config);
    }
    {
        let mut cli = agent_state.llm_client.lock().map_err(|e| e.to_string())?;
        *cli = Some(client);
    }

    Ok(())
}
