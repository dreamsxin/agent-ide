use serde::{Deserialize, Serialize};

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

    pub fn system_prompt(&self) -> &'static str {
        match self {
            AgentRole::Architect => {
                r#"You are an Architect Agent. Your job is:
1. Analyze the user's requirement and current project context.
2. Design the smallest coherent implementation plan.
3. Identify files, interfaces, risks, and validation steps.
4. Hand off implementation-ready guidance to later stages.

Do not write code diffs. Output a concise architecture plan."#
            }
            AgentRole::Coder => {
                r#"You are a Coder Agent. Your job is:
1. Implement code according to the architecture plan and current stage.
2. Keep changes minimal and compatible with the existing codebase.
3. Include error handling and preserve existing behavior.

For code changes, output ONLY diff/new-file blocks in the required Agent IDE format."#
            }
            AgentRole::Tester => {
                r#"You are a Tester Agent. Your job is:
1. Add or adjust focused tests for the implemented behavior.
2. Identify edge cases and likely regressions.
3. Prefer concrete test diffs over general advice.

For code changes, output ONLY diff/new-file blocks in the required Agent IDE format."#
            }
            AgentRole::Reviewer => {
                r#"You are a Reviewer Agent. Your job is:
1. Review the proposed changes for correctness, security, and maintainability.
2. Call out blockers with specific reasoning.
3. Suggest final fixes only when they are concrete and necessary.

Output review findings. Use diff/new-file blocks only for required fixes."#
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    pub role: AgentRole,
    pub name: String,
    pub status: String,
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

pub fn reset_pipeline_status(stages: &[PipelineStage]) -> Vec<PipelineStage> {
    stages
        .iter()
        .map(|stage| PipelineStage {
            role: stage.role,
            name: stage.name.clone(),
            status: "pending".to_string(),
        })
        .collect()
}

pub fn mark_pipeline_stage(stages: &mut [PipelineStage], active_index: usize, status: &str) {
    if let Some(stage) = stages.get_mut(active_index) {
        stage.status = status.to_string();
    }
}

pub fn default_pipeline() -> Vec<PipelineStage> {
    vec![
        PipelineStage::new(AgentRole::Architect, "Design"),
        PipelineStage::new(AgentRole::Coder, "Implement"),
        PipelineStage::new(AgentRole::Tester, "Test"),
        PipelineStage::new(AgentRole::Reviewer, "Review"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_pipeline_status_preserves_roles_and_names() {
        let mut stages = default_pipeline();
        stages[0].status = "completed".to_string();
        stages[1].status = "active".to_string();

        let reset = reset_pipeline_status(&stages);

        assert_eq!(reset.len(), stages.len());
        assert_eq!(reset[0].role, AgentRole::Architect);
        assert_eq!(reset[0].name, "Design");
        assert!(reset.iter().all(|stage| stage.status == "pending"));
    }

    #[test]
    fn mark_pipeline_stage_updates_only_target_stage() {
        let mut stages = default_pipeline();

        mark_pipeline_stage(&mut stages, 1, "active");

        assert_eq!(stages[0].status, "pending");
        assert_eq!(stages[1].status, "active");
        assert_eq!(stages[2].status, "pending");
    }
}
