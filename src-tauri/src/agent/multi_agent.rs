use serde::{Deserialize, Serialize};

/// Agent 角色
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    #[serde(rename = "architect")]
    Architect,
    #[serde(rename = "coder")]
    Coder,
    #[serde(rename = "tester")]
    Tester,
    #[serde(rename = "reviewer")]
    Reviewer,
}

impl AgentRole {
    pub fn to_string(&self) -> &'static str {
        match self {
            AgentRole::Architect => "architect",
            AgentRole::Coder => "coder",
            AgentRole::Tester => "tester",
            AgentRole::Reviewer => "reviewer",
        }
    }

    /// 每个角色的系统提示词
    pub fn system_prompt(&self) -> &'static str {
        match self {
            AgentRole::Architect => r#"You are an Architect Agent. Your job is:
1. Analyze user requirements
2. Design system architecture
3. Break work into well-defined task steps
4. Define interfaces between components
Output your design as a clear plan. Do NOT write implementation code."#,

            AgentRole::Coder => r#"You are a Coder Agent. Your job is:
1. Implement code according to the architecture plan
2. Follow the defined interfaces
3. Write clean, well-structured code
4. Include error handling
Output ONLY executable code or diffs. Be precise and minimal."#,

            AgentRole::Tester => r#"You are a Tester Agent. Your job is:
1. Write unit tests and integration tests
2. Identify edge cases
3. Verify code correctness
4. Report issues with specific file/line references
Output test code and issue reports."#,

            AgentRole::Reviewer => r#"You are a Reviewer Agent. Your job is:
1. Review code for correctness, security, and performance
2. Check adherence to the architecture plan
3. Suggest improvements
4. Approve or request changes
Output a review report with specific recommendations."#,
        }
    }
}

/// 多 Agent 协作流水线阶段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    pub role: AgentRole,
    pub name: String,
    pub status: String, // "pending" | "active" | "done" | "error"
}

impl PipelineStage {
    pub fn new(role: AgentRole, name: &str) -> Self {
        Self {
            role,
            name: name.to_string(),
            status: "pending".to_string(),
        }
    }
}

/// 完整的协作流水线
pub fn default_pipeline() -> Vec<PipelineStage> {
    vec![
        PipelineStage::new(AgentRole::Architect, "Design"),
        PipelineStage::new(AgentRole::Coder, "Implement"),
        PipelineStage::new(AgentRole::Tester, "Test"),
        PipelineStage::new(AgentRole::Reviewer, "Review"),
    ]
}
