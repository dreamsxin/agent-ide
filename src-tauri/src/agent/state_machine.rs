use std::fmt;

/// Agent 状态机 —— 五个核心状态
#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Idle,
    Thinking,
    Planning,
    Acting,
    Reviewing,
    WaitingUser,
    Done,
    Error(String),
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentState::Idle => write!(f, "idle"),
            AgentState::Thinking => write!(f, "thinking"),
            AgentState::Planning => write!(f, "planning"),
            AgentState::Acting => write!(f, "acting"),
            AgentState::Reviewing => write!(f, "reviewing"),
            AgentState::WaitingUser => write!(f, "waiting_user"),
            AgentState::Done => write!(f, "done"),
            AgentState::Error(_) => write!(f, "error"),
        }
    }
}

/// Agent 控制模式
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    Suggest,   // 仅建议
    Edit,      // 可写代码
    Auto,      // 全自动
}

impl fmt::Display for AgentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentMode::Suggest => write!(f, "suggest"),
            AgentMode::Edit => write!(f, "edit"),
            AgentMode::Auto => write!(f, "auto"),
        }
    }
}

/// 状态转换事件
#[derive(Debug)]
pub enum AgentEvent {
    UserPrompt(String),
    PlanReady(Vec<String>),
    StepStart(String),
    StepDone(String),
    ReviewReady,
    UserApply,
    UserReject,
    Error(String),
}
