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

/// Agent 控制模式：改动是等人审查，还是跑完直接落盘。
///
/// 只有两档，因为后端只有一个判据 —— 所有权限门都在问"是不是 Auto"。
/// 曾经有过第三档 `Edit`，它和 `Suggest` 逐位相同：开关给出三个位置却只有两级
/// 权限，用户拨动它什么都不会变。更细的授权由 `WorkspaceToolPermissions`
/// 那几个开关表达（能不能新建文件、能不能跑命令），那里才是真正分级的地方。
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    Suggest,
    Auto,
}


/// IDE work mode. This is separate from the Agent's suggest/auto permission mode.
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

impl AgentMode {
    /// 解析前端传来的模式名。
    ///
    /// 旧版本存过 `"edit"`，它现在不是有效值了。这里不静默当成 `suggest`：
    /// 拨到一个不存在的档位却看不出区别，正是当初要删掉它的原因。
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "suggest" => Ok(AgentMode::Suggest),
            "auto" => Ok(AgentMode::Auto),
            other => Err(format!(
                "Invalid Agent mode: {} (expected suggest or auto)",
                other
            )),
        }
    }
}

impl fmt::Display for AgentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentMode::Suggest => write!(f, "suggest"),
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

impl Default for AgentStateManager {
    fn default() -> Self {
        Self::new()
    }
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
            // 新 prompt 从**任何**状态都直接进 Thinking。
            //
            // 以前只有 `Idle` 和 `Done` 有出边，而且 `Done` 那条是转到 `Idle`
            // ——"先 reset"——但没有任何代码会再补一次 `UserPrompt`。于是后续的
            // `PlanReady`（要求 Thinking）和 `StepStart`（要求 Planning）双双落进
            // 兜底分支，**整次运行都报旧状态**。`WaitingUser` 更常见也更糟：
            // 上一轮产出 diff 还没处理就接着发问，是最普通的用法。
            //
            // 前端据状态决定 Run / Continue 按钮是否可点，所以一次报着 idle 的
            // 活跃运行意味着单步执行的按钮在流水线跑着的时候亮着。
            (_, AgentEvent::UserPrompt(_)) => AgentState::Thinking,
            (AgentState::Thinking, AgentEvent::PlanReady(_)) => AgentState::Planning,
            (AgentState::Planning, AgentEvent::StepStart(_)) => AgentState::Acting,
            (AgentState::Acting, AgentEvent::StepDone(_)) => AgentState::Acting, // 保持
            (AgentState::Acting, AgentEvent::DiffReady(_)) => AgentState::Reviewing,
            (AgentState::Acting, AgentEvent::SddReady(_)) => AgentState::Reviewing,
            (AgentState::Reviewing, _) => AgentState::WaitingUser,
            (AgentState::WaitingUser, AgentEvent::UserApply) => AgentState::Done,
            (AgentState::WaitingUser, AgentEvent::UserReject) => AgentState::Done,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 一次完整的运行必须从任何静息态出发都能走完整条状态链。
    ///
    /// `WaitingUser`（上一轮的 diff 还挂着）和 `Done`（刚 apply 完）都是常见起点，
    /// 而它们以前都走不通：`UserPrompt` 没有出边或者只把状态推回 `Idle`，
    /// 后面的 `PlanReady` / `StepStart` 于是全部落进兜底分支。
    #[test]
    fn a_new_prompt_starts_thinking_from_every_resting_state() {
        for start in [
            AgentState::Idle,
            AgentState::Done,
            AgentState::WaitingUser,
            AgentState::Reviewing,
            AgentState::Error("earlier failure".to_string()),
        ] {
            let mut manager = AgentStateManager::new();
            manager.set(start.clone());

            assert_eq!(
                manager.transition(&AgentEvent::UserPrompt("go".to_string())),
                AgentState::Thinking,
                "从 {:?} 发新 prompt 应当进 Thinking",
                start
            );
            assert_eq!(
                manager.transition(&AgentEvent::PlanReady(Vec::new())),
                AgentState::Planning,
                "从 {:?} 起的运行应当能走到 Planning",
                start
            );
            assert_eq!(
                manager.transition(&AgentEvent::StepStart("stage".to_string())),
                AgentState::Acting,
                "从 {:?} 起的运行应当能走到 Acting",
                start
            );
        }
    }

    #[test]
    fn an_error_wins_over_the_current_state() {
        let mut manager = AgentStateManager::new();
        manager.transition(&AgentEvent::UserPrompt("go".to_string()));

        let state = manager.transition(&AgentEvent::Error("boom".to_string()));

        assert!(matches!(state, AgentState::Error(_)));
    }
}
