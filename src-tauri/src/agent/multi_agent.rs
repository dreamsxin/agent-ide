use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    #[serde(rename = "architect")]
    Architect,
    #[serde(rename = "designer")]
    Designer,
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
            AgentRole::Designer => "designer",
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
            AgentRole::Designer => {
                r#"You are a Designer Agent for Specification Driven Development. Your job is:
1. Convert the user's requirement and planner output into a concrete SDD Markdown draft.
2. Define scope, user flows, interfaces, data contracts, risks, and acceptance criteria.
3. Keep the document implementation-ready without producing source-code diffs.

Output only the requested SDD Markdown draft."#
            }
            AgentRole::Coder => {
                r#"You are a Coder Agent. Your job is:
1. Implement code according to the architecture plan and current stage.
2. Keep changes minimal and compatible with the existing codebase.
3. Include error handling and preserve existing behavior.

For code changes, output ONLY diff/new-file blocks in the required Agent IDE format."#
            }
            AgentRole::Tester => {
                // 这个角色以前只有"Add ... tests"和"Prefer concrete test diffs over
                // general advice"，没有任何不产出的出口。实测「创建 hello.txt 内容为
                // world」被它补出一个 18 行的 test_hello.py —— 职责是写测试的阶段
                // 就一定会写测试。必须显式授权它交白卷。
                r#"You are a Tester Agent. Your job is:
1. Judge first whether the change needs tests at all. Trivial or non-behavioural changes do not.
2. If tests are warranted, add or adjust focused tests for the implemented behaviour.
3. Identify edge cases and likely regressions.

Do not invent test files the user did not ask for and the change does not justify. Writing no
tests is a valid and often correct outcome: say so in one line and produce no diff.

When tests are warranted, output ONLY diff/new-file blocks in the required Agent IDE format."#
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
    #[serde(rename = "pauseBefore", default)]
    pub pause_before: bool,
}

impl PipelineStage {
    pub fn new(role: AgentRole, name: &str) -> Self {
        Self {
            role,
            name: name.to_string(),
            status: "pending".to_string(),
            pause_before: false,
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
            pause_before: stage.pause_before,
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

/// 单点改动用的流水线：只跑实现阶段。
///
/// Architect / Tester / Reviewer 对一行改动只会产出与自己角色同形的输出 ——
/// 一份没人要的设计说明、一个没人要的测试文件、一段没人读的复核意见。
pub fn direct_pipeline() -> Vec<PipelineStage> {
    vec![PipelineStage::new(AgentRole::Coder, "Implement")]
}

/// 这条流水线还是默认那条吗 —— 也就是"用户没有自己配过"。
///
/// 判断"没配过"曾经写成 `pipeline.is_empty()`，而 `AgentGlobalState::new()` 一启动就把
/// 它填成了 `default_pipeline()`，`reset_pipeline` 也设回同一份。于是那个条件永远不成立，
/// `direct_pipeline()` 成了死代码：Code 模式下哪怕只说"改个名字"也要跑满四个阶段、
/// 四次模型调用。空表仍然算"没配过"（CLI 那边可能传空），所以两种都接。
///
/// 只比角色顺序和 `pause_before`，不比 `name`：名字是展示用的（界面还要翻译），
/// 改个显示名不代表用户想换流水线形状；而 `pause_before` 是"这一步跑之前停下来等我"，
/// 那是明确的运行意图，改过就不能再替他裁剪。
pub fn pipeline_matches_default(pipeline: &[PipelineStage]) -> bool {
    if pipeline.is_empty() {
        return true;
    }
    let default = default_pipeline();
    pipeline.len() == default.len()
        && pipeline
            .iter()
            .zip(default.iter())
            .all(|(stage, expected)| {
                stage.role == expected.role && stage.pause_before == expected.pause_before
            })
}

pub fn plan_pipeline() -> Vec<PipelineStage> {
    vec![
        PipelineStage::new(AgentRole::Designer, "Draft SDD"),
        PipelineStage::new(AgentRole::Reviewer, "Review SDD"),
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
        stages[2].pause_before = true;

        let reset = reset_pipeline_status(&stages);

        assert_eq!(reset.len(), stages.len());
        assert_eq!(reset[0].role, AgentRole::Architect);
        assert_eq!(reset[0].name, "Design");
        assert!(reset[2].pause_before);
        assert!(reset.iter().all(|stage| stage.status == "pending"));
    }

    #[test]
    fn plan_pipeline_uses_designer_before_reviewer() {
        let stages = plan_pipeline();

        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].role, AgentRole::Designer);
        assert_eq!(stages[1].role, AgentRole::Reviewer);
    }

    #[test]
    fn mark_pipeline_stage_updates_only_target_stage() {
        let mut stages = default_pipeline();

        mark_pipeline_stage(&mut stages, 1, "active");

        assert_eq!(stages[0].status, "pending");
        assert_eq!(stages[1].status, "active");
        assert_eq!(stages[2].status, "pending");
    }

    #[test]
    fn pipeline_matches_default_recognises_the_default() {
        // 默认的 4 个阶段 —— 无论 status 是什么，只看 role 和 pause_before
        let mut stages = default_pipeline();
        stages[0].status = "completed".to_string();
        stages[1].name = "改了个名字".to_string();
        assert!(pipeline_matches_default(&stages));
    }

    #[test]
    fn pipeline_matches_default_rejects_custom_pipelines() {
        // 加了一步
        let mut longer = default_pipeline();
        longer.push(PipelineStage::new(AgentRole::Designer, "Extra"));
        assert!(!pipeline_matches_default(&longer));

        // 换了角色顺序
        let mut swapped = default_pipeline();
        swapped.swap(0, 1);
        assert!(!pipeline_matches_default(&swapped));

        // 开了 pause_before
        let mut paused = default_pipeline();
        paused[2].pause_before = true;
        assert!(!pipeline_matches_default(&paused));
    }

    #[test]
    fn empty_pipeline_counts_as_default() {
        assert!(pipeline_matches_default(&[]));
    }
}
