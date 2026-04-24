use crate::agent::state_machine::{AgentState, AgentMode};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

/// Agent 全局状态（Arc 包裹确保 async 安全）
#[derive(Clone)]
pub struct AgentGlobalState {
    pub state: Arc<Mutex<AgentState>>,
    pub mode: Arc<Mutex<AgentMode>>,
}

impl AgentGlobalState {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AgentState::Idle)),
            mode: Arc::new(Mutex::new(AgentMode::Suggest)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
    pub context_files: Vec<String>,
}

/// 获取 Agent 当前状态
#[tauri::command]
pub fn get_agent_state(agent_state: State<AgentGlobalState>) -> Result<AgentStatus, String> {
    let state = agent_state.state.lock().map_err(|e| e.to_string())?;
    let mode = agent_state.mode.lock().map_err(|e| e.to_string())?;

    Ok(AgentStatus {
        state: state.to_string(),
        mode: mode.to_string(),
        context_files: Vec::new(),
    })
}

/// 发送 Prompt 到 Agent（占位，后续对接 LLM）
#[tauri::command]
pub async fn send_agent_prompt(
    prompt: String,
    context_files: Vec<String>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    // 更新状态为 Thinking
    {
        let mut state = agent_state.state.lock().map_err(|e| e.to_string())?;
        *state = AgentState::Thinking;
    }

    // TODO: 实际对接 LLM 调用，通过 Tauri Event 推送流式响应
    // TODO: 实现完整状态机转换: Thinking → Planning → Acting → Reviewing

    // 模拟处理后返回
    {
        let mut state = agent_state.state.lock().map_err(|e| e.to_string())?;
        *state = AgentState::Idle;
    }

    Ok(format!(
        "Agent received prompt: {}. Context files: {:?}",
        prompt, context_files
    ))
}

/// 停止 Agent 当前任务
#[tauri::command]
pub fn stop_agent(agent_state: State<AgentGlobalState>) -> Result<String, String> {
    let mut state = agent_state.state.lock().map_err(|e| e.to_string())?;
    *state = AgentState::Idle;
    Ok("Agent stopped".to_string())
}
