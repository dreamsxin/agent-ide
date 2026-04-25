use std::fmt;

/// Agent 状态枚举
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
    Suggest,
    Edit,
    Auto,
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
    PlanReady(Vec<TaskStep>),
    StepStart(String),
    StepDone(String),
    DiffReady(Vec<FileDiff>),
    UserApply,
    UserReject,
    Error(String),
}

/// 任务步骤（可序列化，用于跨 IPC 传输）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskStep {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub step_type: String,
    pub status: String,
    pub logs: Vec<String>,
}

/// 文件 Diff（可序列化，用于跨 IPC 传输）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileDiff {
    pub id: String,
    pub file: String,
    pub hunks: Vec<DiffHunk>,
    pub status: String,
}

/// Diff Hunk
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffHunk {
    #[serde(rename = "oldStart")]
    pub old_start: u32,
    #[serde(rename = "oldLines")]
    pub old_lines: u32,
    #[serde(rename = "newStart")]
    pub new_start: u32,
    #[serde(rename = "newLines")]
    pub new_lines: u32,
    pub content: String,
    /// 原始代码块（用于应用 diff 时替换定位）
    pub original: String,
    /// 更新后的代码块
    pub updated: String,
}

/// Agent 状态管理器 —— 封装状态转换逻辑
pub struct AgentStateManager {
    pub state: AgentState,
}

impl AgentStateManager {
    pub fn new() -> Self {
        Self {
            state: AgentState::Idle,
        }
    }

    /// 处理事件，执行状态转换。返回新的状态和可选的 transition 事件数据。
    pub fn transition(&mut self, event: &AgentEvent) -> AgentState {
        self.state = match (&self.state, event) {
            (AgentState::Idle, AgentEvent::UserPrompt(_)) => AgentState::Thinking,
            (AgentState::Thinking, AgentEvent::PlanReady(_)) => AgentState::Planning,
            (AgentState::Planning, AgentEvent::StepStart(_)) => AgentState::Acting,
            (AgentState::Acting, AgentEvent::StepDone(_)) => AgentState::Acting, // 保持
            (AgentState::Acting, AgentEvent::DiffReady(_)) => AgentState::Reviewing,
            (AgentState::Reviewing, _) => AgentState::WaitingUser,
            (AgentState::WaitingUser, AgentEvent::UserApply) => AgentState::Done,
            (AgentState::WaitingUser, AgentEvent::UserReject) => AgentState::Done,
            (AgentState::Done, AgentEvent::UserPrompt(_)) => AgentState::Idle, // 先 reset
            (_, AgentEvent::Error(_)) => AgentState::Error(String::new()),
            _ => self.state.clone(),
        };
        self.state.clone()
    }

    /// 直接设置状态（用于外部控制，如 stop）
    pub fn set(&mut self, state: AgentState) {
        self.state = state;
    }
}
