use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::executor;
use crate::agent::multi_agent::{mark_pipeline_stage, reset_pipeline_status, PipelineStage};
use crate::agent::planner;
use crate::agent::state_machine::{AgentMode, AgentStateManager, TaskStep};
use crate::services::context::{AgentContext, ContextCompressionMode};
use crate::services::llm_client::LlmClient;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct ActionLogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub phase: String,
    pub role: Option<String>,
    pub stage: Option<String>,
    pub summary: String,
    pub details: String,
    #[serde(rename = "contextSummary")]
    pub context_summary: Option<String>,
    #[serde(rename = "diffSummary")]
    pub diff_summary: Option<String>,
}

/// Agent orchestrator - main flow controller.
pub struct AgentOrchestrator {
    pub state_mgr: AgentStateManager,
    pub mode: AgentMode,
    pub steps: Vec<TaskStep>,
    pub diffs: Vec<crate::agent::state_machine::FileDiff>,
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {
            state_mgr: AgentStateManager::new(),
            mode: AgentMode::Suggest,
            steps: Vec::new(),
            diffs: Vec::new(),
        }
    }

    /// Run the full Agent flow:
    /// prompt -> LLM plan -> execute steps -> generate diffs -> await user
    pub async fn run(
        &mut self,
        prompt: String,
        context: AgentContext,
        context_compression: ContextCompressionMode,
        pipeline: Vec<PipelineStage>,
        cancel_flag: Arc<AtomicBool>,
        llm: &LlmClient,
        app: AppHandle,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        // 1. Transition to Thinking
        let _ = self
            .state_mgr
            .transition(&AgentEvent::UserPrompt(prompt.clone()));
        self.emit_state(&app);

        // 2. Call LLM Streaming for planning
        let ctx_str = context.to_prompt_context_with_mode(&context_compression);
        let context_summary = summarize_text(&ctx_str, 600);
        self.emit_action_log(
            &app,
            "info",
            "prompt",
            None,
            None,
            "Agent prompt received",
            &format!("Prompt:\n{}\n\nContext mode: {}", prompt, context_compression.to_string()),
            Some(context_summary.clone()),
            None,
        );
        let mut pipeline = if pipeline.is_empty() {
            reset_pipeline_status(&crate::agent::multi_agent::default_pipeline())
        } else {
            reset_pipeline_status(&pipeline)
        };
        self.emit_pipeline(&app, &pipeline);
        let (tx, mut rx) = mpsc::channel::<String>(32);

        // Forward planner stream tokens to the frontend.
        let app_clone = app.clone();
        tokio::spawn(async move {
            while let Some(token) = rx.recv().await {
                let _ = app_clone.emit("agent-stream-token", token);
            }
        });

        let (steps, _full_response) =
            planner::plan_task(llm, &prompt, &ctx_str, cancel_flag.clone(), tx).await?;
        self.emit_action_log(
            &app,
            "success",
            "planner",
            None,
            Some("Planner"),
            &format!("Planner produced {} step{}", steps.len(), if steps.len() == 1 { "" } else { "s" }),
            &_full_response,
            Some(context_summary.clone()),
            None,
        );

        self.steps = steps;
        self.ensure_not_cancelled(&cancel_flag, &app)?;

        // 3. Transition to Planning
        let _ = self
            .state_mgr
            .transition(&AgentEvent::PlanReady(self.steps.clone()));
        self.emit_state(&app);
        let _ = app.emit(
            "agent-plan-ready",
            serde_json::to_value(&self.steps).unwrap_or_default(),
        );

        // 4. Execute the configured role pipeline.
        let mut stage_outputs: Vec<String> = vec![format!("Planner:\n{}", _full_response)];
        for stage_index in 0..pipeline.len() {
            let stage = pipeline[stage_index].clone();
            mark_pipeline_stage(&mut pipeline, stage_index, "active");
            self.emit_pipeline(&app, &pipeline);
            self.emit_action_log(
                &app,
                "info",
                "stage_start",
                Some(stage.role.to_string()),
                Some(&stage.name),
                &format!("{} stage started", stage.name),
                &format!("Role: {}\nStage index: {}", stage.role.to_string(), stage_index + 1),
                Some(context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );

            let step_index = self.ensure_stage_step(&stage);
            self.steps[step_index].status = "doing".to_string();
            self.steps[step_index]
                .logs
                .push(format!("{} stage started", stage.role.to_string()));
            self.emit_step(&app, step_index);

            let _ = self
                .state_mgr
                .transition(&AgentEvent::StepStart(stage.name.clone()));
            self.emit_state(&app);

            let (tx2, mut rx2) = mpsc::channel::<String>(32);
            let app_clone2 = app.clone();
            tokio::spawn(async move {
                while let Some(token) = rx2.recv().await {
                    let _ = app_clone2.emit("agent-stream-token", token);
                }
            });

            let prior_outputs = stage_outputs.join("\n\n---\n\n");
            let pending_diff_summary = self.summarize_pending_diffs();

            match executor::execute_stage(
                llm,
                stage.role,
                &stage.name,
                &prompt,
                &ctx_str,
                &prior_outputs,
                &pending_diff_summary,
                cancel_flag.clone(),
                tx2,
            )
                .await
            {
                Ok(response) => {
                    self.steps[step_index].status = "done".to_string();
                    self.steps[step_index].logs.push(format!(
                        "{} response: {}...",
                        stage.role.to_string(),
                        response.chars().take(200).collect::<String>()
                    ));
                    stage_outputs.push(format!(
                        "{} / {}:\n{}",
                        stage.name,
                        stage.role.to_string(),
                        response
                    ));

                    let step_diffs = executor::parse_diffs(&response);
                    let generated_diff_count = step_diffs.len();
                    self.diffs.extend(step_diffs);
                    mark_pipeline_stage(&mut pipeline, stage_index, "completed");
                    self.emit_action_log(
                        &app,
                        "success",
                        "stage_complete",
                        Some(stage.role.to_string()),
                        Some(&stage.name),
                        &format!(
                            "{} stage completed with {} new diff{}",
                            stage.name,
                            generated_diff_count,
                            if generated_diff_count == 1 { "" } else { "s" }
                        ),
                        &response,
                        Some(context_summary.clone()),
                        Some(self.summarize_pending_diffs()),
                    );
                }
                Err(e) => {
                    self.steps[step_index].status = "error".to_string();
                    self.steps[step_index].logs.push(format!("Error: {}", e));
                    mark_pipeline_stage(&mut pipeline, stage_index, "failed");
                    self.emit_step(&app, step_index);
                    self.emit_pipeline(&app, &pipeline);
                    self.emit_action_log(
                        &app,
                        "error",
                        "stage_error",
                        Some(stage.role.to_string()),
                        Some(&stage.name),
                        &format!("{} stage failed", stage.name),
                        &e,
                        Some(context_summary.clone()),
                        Some(self.summarize_pending_diffs()),
                    );
                    return Err(e);
                }
            }

            self.ensure_not_cancelled(&cancel_flag, &app)?;
            self.emit_step(&app, step_index);
            self.emit_pipeline(&app, &pipeline);

            let _ = self
                .state_mgr
                .transition(&AgentEvent::StepDone(stage.name.clone()));
            self.emit_state(&app);

            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }

        // 5. Auto applies diffs immediately; other modes wait for review.
        if !self.diffs.is_empty() {
            let _ = app.emit(
                "agent-diff-ready",
                serde_json::to_value(&self.diffs).unwrap_or_default(),
            );
            self.emit_action_log(
                &app,
                "info",
                "diff_ready",
                None,
                None,
                &format!("{} pending diff{} ready for review", self.pending_diff_count(), if self.pending_diff_count() == 1 { "" } else { "s" }),
                "Diff review is waiting for user action.",
                Some(context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
        }
        let _ = self
            .state_mgr
            .transition(&AgentEvent::DiffReady(self.diffs.clone()));

        if self.mode == AgentMode::Auto {
            // Auto mode applies diffs immediately.
            self.apply_diffs_to_fs()?;
            self.emit_action_log(
                &app,
                "success",
                "auto_apply",
                None,
                None,
                "Auto mode applied pending diffs",
                "Agent auto mode completed filesystem apply.",
                Some(context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::Done);
        } else {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
        }
        self.emit_state(&app);

        Ok(())
    }

    /// Apply pending diffs to the workspace filesystem.
    pub fn apply_diffs_to_fs(&mut self) -> Result<(), String> {
        let result = apply_pending_diffs(&self.diffs);

        for diff in &mut self.diffs {
            if result.applied.iter().any(|item| item.id == diff.id) {
                diff.status = "applied".to_string();
            } else if result.failed.iter().any(|item| item.diff_id == diff.id) {
                diff.status = "failed".to_string();
            }
        }

        if !result.failed.is_empty() {
            return Err(result
                .failed
                .iter()
                .map(|item| format!("{}: {}", item.file, item.message))
                .collect::<Vec<_>>()
                .join("; "));
        }

        Ok(())
    }

    /// Emit the current state to the frontend.
    fn emit_state(&self, app: &AppHandle) {
        let payload = serde_json::json!({
            "state": self.state_mgr.state.to_string(),
            "mode": self.mode.to_string(),
        });
        let _ = app.emit("agent-state-changed", payload);
    }

    fn emit_pipeline(&self, app: &AppHandle, pipeline: &[PipelineStage]) {
        let _ = app.emit(
            "agent-pipeline-update",
            serde_json::to_value(pipeline).unwrap_or_default(),
        );
    }

    fn emit_step(&self, app: &AppHandle, step_index: usize) {
        if let Some(step) = self.steps.get(step_index) {
            let _ = app.emit(
                "agent-step-update",
                serde_json::to_value(step).unwrap_or_default(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_action_log(
        &self,
        app: &AppHandle,
        level: &str,
        phase: &str,
        role: Option<&str>,
        stage: Option<&str>,
        summary: &str,
        details: &str,
        context_summary: Option<String>,
        diff_summary: Option<String>,
    ) {
        let entry = ActionLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            phase: phase.to_string(),
            role: role.map(str::to_string),
            stage: stage.map(str::to_string),
            summary: summary.to_string(),
            details: details.to_string(),
            context_summary,
            diff_summary,
        };
        let _ = app.emit("agent-action-log", entry);
    }

    fn pending_diff_count(&self) -> usize {
        self.diffs
            .iter()
            .filter(|diff| diff.status == "pending")
            .count()
    }

    fn summarize_pending_diffs(&self) -> String {
        let pending: Vec<_> = self
            .diffs
            .iter()
            .filter(|diff| diff.status == "pending")
            .collect();

        if pending.is_empty() {
            return "No pending diffs.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Pending diffs: {}", pending.len()));
        for diff in pending {
            lines.push(format!(
                "- {}: {} hunk{}",
                diff.file,
                diff.hunks.len(),
                if diff.hunks.len() == 1 { "" } else { "s" }
            ));
            for (index, hunk) in diff.hunks.iter().enumerate() {
                lines.push(format!(
                    "  Hunk {}: -{} lines, +{} lines",
                    index + 1,
                    hunk.old_lines,
                    hunk.new_lines
                ));
                if !hunk.original.trim().is_empty() {
                    lines.push(format!(
                        "  Original excerpt: {}",
                        summarize_text(&hunk.original, 180)
                    ));
                }
                if !hunk.updated.trim().is_empty() {
                    lines.push(format!(
                        "  Updated excerpt: {}",
                        summarize_text(&hunk.updated, 180)
                    ));
                }
            }
        }
        lines.join("\n")
    }

    fn ensure_stage_step(&mut self, stage: &PipelineStage) -> usize {
        if let Some(index) = self.steps.iter().position(|step| step.title == stage.name) {
            return index;
        }

        self.steps.push(TaskStep {
            id: uuid::Uuid::new_v4().to_string(),
            title: stage.name.clone(),
            step_type: stage.role.to_string().to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
        });
        self.steps.len() - 1
    }

    fn ensure_not_cancelled(
        &mut self,
        cancel_flag: &Arc<AtomicBool>,
        app: &AppHandle,
    ) -> Result<(), String> {
        if cancel_flag.load(Ordering::SeqCst) {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::Idle);
            self.emit_state(app);
            return Err("Agent task cancelled".to_string());
        }
        Ok(())
    }

    /// Mark all pending diffs as applied.
    pub fn apply_diffs(&mut self) {
        for diff in &mut self.diffs {
            if diff.status == "pending" {
                diff.status = "applied".to_string();
            }
        }
    }

    /// Mark all pending diffs as rejected.
    pub fn reject_diffs(&mut self) {
        for diff in &mut self.diffs {
            if diff.status == "pending" {
                diff.status = "rejected".to_string();
            }
        }
    }
}

fn summarize_text(text: &str, max_chars: usize) -> String {
    let normalized = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let mut summary: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        summary.push_str("...");
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state_machine::{DiffHunk, FileDiff};
    use crate::services::workspace;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = std::env::temp_dir()
                .join(format!("agent-ide-orchestrator-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write_file(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(
                self.root
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.root.clone()),
            );
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    fn make_diff(file: &str, original: &str, updated: &str) -> FileDiff {
        FileDiff {
            id: Uuid::new_v4().to_string(),
            file: file.to_string(),
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: original.to_string(),
                updated: updated.to_string(),
            }],
            status: "pending".to_string(),
        }
    }

    #[test]
    fn auto_apply_marks_partial_failure_and_returns_error() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("ok.ts", "const value = 1;\n");
        env.write_file("fail.ts", "const other = 1;\n");

        let ok = make_diff("ok.ts", "const value = 1;", "const value = 2;");
        let fail = make_diff("fail.ts", "const missing = 1;", "const missing = 2;");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![ok.clone(), fail.clone()];

        let err = orchestrator.apply_diffs_to_fs().unwrap_err();

        assert!(err.contains("Could not find original content"));
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(orchestrator.diffs[1].status, "failed");
        assert_eq!(
            std::fs::read_to_string(env.root.join("ok.ts")).unwrap(),
            "const value = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("fail.ts")).unwrap(),
            "const other = 1;\n"
        );
    }

    #[test]
    fn summarize_pending_diffs_includes_actual_pending_diff_context() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![
            make_diff("src/app.ts", "const oldValue = 1;", "const newValue = 2;"),
            FileDiff {
                status: "applied".to_string(),
                ..make_diff("src/done.ts", "done()", "done(true)")
            },
        ];

        let summary = orchestrator.summarize_pending_diffs();

        assert!(summary.contains("Pending diffs: 1"));
        assert!(summary.contains("src/app.ts"));
        assert!(summary.contains("Original excerpt: const oldValue = 1;"));
        assert!(summary.contains("Updated excerpt: const newValue = 2;"));
        assert!(!summary.contains("src/done.ts"));
    }
}
