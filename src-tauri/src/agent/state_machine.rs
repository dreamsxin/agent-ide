use std::collections::BTreeMap;
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

/// IDE work mode. This is separate from Agent permissions such as suggest/edit/auto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeMode {
    Code,
    Plan,
}

impl IdeMode {
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "code" => Ok(IdeMode::Code),
            "plan" => Ok(IdeMode::Plan),
            other => Err(format!("Invalid IDE mode: {}", other)),
        }
    }
}

impl fmt::Display for IdeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdeMode::Code => write!(f, "code"),
            IdeMode::Plan => write!(f, "plan"),
        }
    }
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
    SddReady(SddArtifact),
    UserApply,
    UserReject,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SddArtifact {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub frontmatter: BTreeMap<String, String>,
    pub markdown: String,
    #[serde(rename = "sourceRunId")]
    pub source_run_id: Option<String>,
    #[serde(rename = "reviewFindings", default)]
    pub review_findings: Vec<String>,
    pub status: String,
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
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(rename = "executionMode", default)]
    pub execution_mode: Option<String>,
}

/// 文件 Diff（可序列化，用于跨 IPC 传输）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileDiff {
    pub id: String,
    pub file: String,
    #[serde(rename = "baseHash", default)]
    pub base_hash: Option<String>,
    #[serde(default)]
    pub provenance: Option<DiffProvenance>,
    pub hunks: Vec<DiffHunk>,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffProvenance {
    pub protocol: String,
    pub operation: String,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: Option<u32>,
    #[serde(rename = "changeIndex", default)]
    pub change_index: Option<usize>,
    #[serde(rename = "sourceRole", default)]
    pub source_role: Option<String>,
    #[serde(rename = "sourceStage", default)]
    pub source_stage: Option<String>,
    #[serde(rename = "regeneratedFromDiffId", default)]
    pub regenerated_from_diff_id: Option<String>,
    #[serde(rename = "regeneratedFromHunkIndex", default)]
    pub regenerated_from_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApplyDiffError {
    #[serde(rename = "diffId")]
    pub diff_id: String,
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApplyDiffsResult {
    pub applied: Vec<FileDiff>,
    pub failed: Vec<ApplyDiffError>,
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
    #[serde(default)]
    pub provenance: Option<DiffHunkProvenance>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffHunkProvenance {
    #[serde(rename = "changeIndex", default)]
    pub change_index: Option<usize>,
    #[serde(rename = "hunkIndex", default)]
    pub hunk_index: Option<usize>,
    #[serde(rename = "sourceRole", default)]
    pub source_role: Option<String>,
    #[serde(rename = "sourceStage", default)]
    pub source_stage: Option<String>,
    #[serde(rename = "promptContext", default)]
    pub prompt_context: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
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
            (AgentState::Acting, AgentEvent::SddReady(_)) => AgentState::Reviewing,
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
