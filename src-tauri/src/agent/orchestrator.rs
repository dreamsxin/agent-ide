use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::executor;
use crate::agent::multi_agent::{
    default_pipeline, mark_pipeline_stage, plan_pipeline, reset_pipeline_status, AgentRole,
    PipelineStage,
};
use crate::agent::planner;
use crate::agent::state_machine::{
    AgentMode, AgentStateManager, DiffHunkProvenance, DiffProvenance, IdeMode, SddArtifact,
    TaskStep,
};
use crate::services::context::{
    estimated_input_tokens_from_budget, AgentContext, ContextBudget, ContextBuildOptions,
    ContextCompressionMode, ContextSourceOptions,
};
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
    pub ide_mode: IdeMode,
    pub steps: Vec<TaskStep>,
    pub diffs: Vec<crate::agent::state_machine::FileDiff>,
    pub sdd_artifacts: Vec<SddArtifact>,
    pub current_run_id: Option<String>,
    pub last_run_id: Option<String>,
    pub paused_run: Option<PausedPipelineRun>,
    /// 外部工具执行器（MCP）。None 表示本次运行不暴露外部工具。
    pub tool_invoker: Option<Arc<dyn crate::agent::executor::ToolInvoker>>,
    /// Auto 模式自动应用时是否允许创建新文件。
    ///
    /// 保守默认 false：请求没带这个权限时，新建文件的 diff 留给人工审查，
    /// 而不是被静默写盘。编辑已有文件不受影响。
    pub allow_file_create: bool,
    /// 本次运行的 token 记账器。
    ///
    /// 存在 orchestrator 上而不是只存在命令的局部变量里，是为了让 `continue_agent_pipeline`
    /// 恢复暂停的运行时能接着用同一个额度：否则续跑会重新从 0 记账，配置的
    /// 单次运行上限只要中途暂停一次就形同虚设。
    pub run_usage: Option<Arc<crate::services::llm_client::RunUsageMeter>>,
    /// 本会话里已经完成的几轮对话，最新的在最后。
    ///
    /// 没有它的话每次 prompt 都是冷启动 —— 跟进一句"再处理下错误分支"读不到
    /// 上一轮做了什么。只保留末尾若干轮并且每条都截断：这里要的是"上次干了啥"
    /// 的线索，不是完整逐字记录，后者会把上下文预算吃光。
    pub conversation: Vec<ConversationTurn>,
}

/// 一轮已完成的对话：用户说了什么，以及那一轮的结果
#[derive(Clone, Debug, Serialize)]
pub struct ConversationTurn {
    pub prompt: String,
    pub outcome: String,
}

/// 保留的对话轮数
const MAX_CONVERSATION_TURNS: usize = 6;
/// 每轮 prompt / 结果各自的字符上限
const MAX_TURN_PROMPT_CHARS: usize = 400;
const MAX_TURN_OUTCOME_CHARS: usize = 300;

#[derive(Debug, Clone)]
pub struct PausedPipelineRun {
    pub prompt: String,
    pub context: String,
    pub context_summary: String,
    pub stage_outputs: Vec<String>,
    pub pipeline: Vec<PipelineStage>,
    pub stage_index: usize,
    pub ide_mode: IdeMode,
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {
            state_mgr: AgentStateManager::new(),
            mode: AgentMode::Suggest,
            ide_mode: IdeMode::Code,
            steps: Vec::new(),
            diffs: Vec::new(),
            sdd_artifacts: Vec::new(),
            current_run_id: None,
            last_run_id: None,
            paused_run: None,
            tool_invoker: None,
            allow_file_create: false,
            run_usage: None,
            conversation: Vec::new(),
        }
    }

    /// 之前几轮的摘要，喂回下一次运行的上下文；没有历史时返回 None
    pub fn conversation_digest(&self) -> Option<String> {
        if self.conversation.is_empty() {
            return None;
        }
        let digest = self
            .conversation
            .iter()
            .enumerate()
            .map(|(index, turn)| {
                format!(
                    "{}. asked: {}\n   result: {}",
                    index + 1,
                    turn.prompt,
                    turn.outcome
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(digest)
    }

    /// 记一轮已完成的对话。结果由当前状态推导，命令层不需要拼摘要。
    pub fn record_conversation_turn(&mut self, prompt: &str) {
        let reviewable: Vec<&str> = self
            .diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .map(|diff| diff.file.as_str())
            .collect();
        let applied = self
            .diffs
            .iter()
            .filter(|diff| diff.status == "applied")
            .count();

        let outcome = if reviewable.is_empty() && applied == 0 {
            "no file changes produced".to_string()
        } else {
            let mut parts = Vec::new();
            if applied > 0 {
                parts.push(format!("{} file(s) applied", applied));
            }
            if !reviewable.is_empty() {
                parts.push(format!("awaiting review: {}", reviewable.join(", ")));
            }
            parts.join("; ")
        };

        self.conversation.push(ConversationTurn {
            prompt: summarize_text(prompt.trim(), MAX_TURN_PROMPT_CHARS),
            outcome: summarize_text(&outcome, MAX_TURN_OUTCOME_CHARS),
        });
        // 只留末尾若干轮：早期的轮次对"接着上一句"没什么帮助，却一直占预算
        if self.conversation.len() > MAX_CONVERSATION_TURNS {
            let excess = self.conversation.len() - MAX_CONVERSATION_TURNS;
            self.conversation.drain(..excess);
        }
    }

    /// 开始新任务时清空对话历史
    pub fn clear_conversation(&mut self) {
        self.conversation.clear();
    }

    /// 开始一次新运行：换 run id
    pub fn begin_run(&mut self, run_id: Option<String>) {
        self.current_run_id = run_id.clone();
        self.last_run_id = run_id;
    }

    /// 开启一个新的用量记账周期（一次全新运行）
    pub fn start_usage_accounting(
        &mut self,
        meter: Arc<crate::services::llm_client::RunUsageMeter>,
    ) {
        self.run_usage = Some(meter);
    }

    /// 恢复暂停的运行时沿用的记账器；没有可沿用的就返回 None
    pub fn resumed_usage_meter(&self) -> Option<Arc<crate::services::llm_client::RunUsageMeter>> {
        self.run_usage.clone()
    }

    pub fn finish_run(&mut self) {
        self.current_run_id = None;
    }

    /// Run the full Agent flow:
    /// prompt -> LLM plan -> execute steps -> generate diffs -> await user
    pub async fn run(
        &mut self,
        prompt: String,
        context: AgentContext,
        context_compression: ContextCompressionMode,
        context_budget: Option<ContextBudget>,
        context_sources: ContextSourceOptions,
        pipeline: Vec<PipelineStage>,
        ide_mode: IdeMode,
        cancel_flag: Arc<AtomicBool>,
        llm: &LlmClient,
        app: AppHandle,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        self.ide_mode = ide_mode;
        // 1. Transition to Thinking
        let _ = self
            .state_mgr
            .transition(&AgentEvent::UserPrompt(prompt.clone()));
        self.emit_state(&app);

        // 2. Call LLM Streaming for planning
        let raw_ctx_str = context.to_prompt_context_with_mode(&context_compression);
        let ctx_str = context.to_prompt_context_with_options(&ContextBuildOptions::new(
            context_compression.clone(),
            context_budget.clone(),
        ));
        let context_summary = summarize_text(&ctx_str, 600);
        let budget_summary = format_context_budget_summary(
            context_budget.as_ref(),
            raw_ctx_str.len(),
            ctx_str.len(),
        );
        self.emit_action_log(
            &app,
            "info",
            "prompt",
            None,
            None,
            "Agent prompt received",
            &format!(
                "Prompt:\n{}\n\nContext mode: {}\n{}\n{}",
                prompt,
                context_compression,
                budget_summary,
                format_context_sources(&context_sources)
            ),
            Some(context_summary.clone()),
            None,
        );
        let pipeline = if ide_mode == IdeMode::Plan {
            reset_pipeline_status(&plan_pipeline())
        } else if pipeline.is_empty() {
            reset_pipeline_status(&default_pipeline())
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
            &format!(
                "Planner produced {} step{}",
                steps.len(),
                if steps.len() == 1 { "" } else { "s" }
            ),
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
        let stage_outputs: Vec<String> = vec![format!("Planner:\n{}", _full_response)];
        self.continue_pipeline_from(
            prompt,
            ctx_str,
            context_summary,
            pipeline,
            stage_outputs,
            0,
            false,
            ide_mode,
            cancel_flag,
            llm,
            app,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn continue_pipeline_from(
        &mut self,
        prompt: String,
        ctx_str: String,
        context_summary: String,
        mut pipeline: Vec<PipelineStage>,
        mut stage_outputs: Vec<String>,
        start_index: usize,
        ignore_pause_once: bool,
        ide_mode: IdeMode,
        cancel_flag: Arc<AtomicBool>,
        llm: &LlmClient,
        app: AppHandle,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        self.ide_mode = ide_mode;
        for stage_index in start_index..pipeline.len() {
            let stage = pipeline[stage_index].clone();
            if stage.pause_before && !(ignore_pause_once && stage_index == start_index) {
                mark_pipeline_stage(&mut pipeline, stage_index, "paused");
                self.paused_run = Some(PausedPipelineRun {
                    prompt: prompt.clone(),
                    context: ctx_str.clone(),
                    context_summary: context_summary.clone(),
                    stage_outputs: stage_outputs.clone(),
                    pipeline: pipeline.clone(),
                    stage_index,
                    ide_mode,
                });
                self.emit_pipeline(&app, &pipeline);
                self.emit_action_log(
                    &app,
                    "info",
                    "stage_paused",
                    Some(stage.role.to_string()),
                    Some(&stage.name),
                    &format!("Paused before {}", stage.name),
                    "Pipeline paused before this stage by user configuration. Disable pause before this stage and rerun or continue with single-step controls.",
                    Some(context_summary.clone()),
                    Some(self.summarize_pending_diffs()),
                );
                self.state_mgr
                    .set(crate::agent::state_machine::AgentState::WaitingUser);
                self.emit_state(&app);
                return Ok(());
            }
            mark_pipeline_stage(&mut pipeline, stage_index, "active");
            self.emit_pipeline(&app, &pipeline);
            self.emit_action_log(
                &app,
                "info",
                "stage_start",
                Some(stage.role.to_string()),
                Some(&stage.name),
                &format!("{} stage started", stage.name),
                &format!(
                    "Role: {}\nStage index: {}",
                    stage.role.to_string(),
                    stage_index + 1
                ),
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
                self.tool_invoker.as_deref(),
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

                    let generated_diff_count = if ide_mode == IdeMode::Plan {
                        self.handle_plan_stage_response(
                            &app,
                            &stage,
                            &response,
                            &prompt,
                            context_summary.clone(),
                        )
                    } else {
                        let parsed = executor::parse_diffs_with_diagnostics(&response);
                        let mut step_diffs = parsed.diffs;
                        attach_stage_provenance(
                            &mut step_diffs,
                            stage.role.to_string(),
                            &stage.name,
                        );
                        // 生成时记录目标文件的内容指纹，apply 时才能识别期间发生的外部改动
                        crate::agent::diff_apply::stamp_base_hashes(&mut step_diffs);
                        let generated_diff_count = step_diffs.len();
                        self.diffs.extend(step_diffs);
                        if !parsed.diagnostics.is_empty() {
                            self.emit_action_log(
                                &app,
                                "warn",
                                "agent_changes_validation",
                                Some(stage.role.to_string()),
                                Some(&stage.name),
                                "Agent changes validation reported issues",
                                &parsed.diagnostics.join("\n"),
                                Some(context_summary.clone()),
                                Some(self.summarize_pending_diffs()),
                            );
                        }
                        generated_diff_count
                    };
                    mark_pipeline_stage(&mut pipeline, stage_index, "completed");
                    self.emit_action_log(
                        &app,
                        "success",
                        "stage_complete",
                        Some(stage.role.to_string()),
                        Some(&stage.name),
                        &format!(
                            "{} stage completed with {} new {}{}",
                            stage.name,
                            generated_diff_count,
                            if ide_mode == IdeMode::Plan {
                                "artifact"
                            } else {
                                "diff"
                            },
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
        if ide_mode == IdeMode::Plan {
            if let Some(artifact) = self.sdd_artifacts.last() {
                let _ = app.emit(
                    "agent-sdd-ready",
                    serde_json::to_value(artifact).unwrap_or_default(),
                );
                self.emit_action_log(
                    &app,
                    "info",
                    "sdd_ready",
                    None,
                    None,
                    &format!("SDD draft ready: {}", artifact.title),
                    "Plan mode completed without producing file diffs.",
                    Some(context_summary.clone()),
                    None,
                );
            }
            if let Some(artifact) = self.sdd_artifacts.last().cloned() {
                let _ = self.state_mgr.transition(&AgentEvent::SddReady(artifact));
            }
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
            self.emit_state(&app);
            return Ok(());
        }

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
                &format!(
                    "{} pending diff{} ready for review",
                    self.pending_diff_count(),
                    if self.pending_diff_count() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ),
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
            let blocked = self.apply_diffs_to_fs()?;
            let (level, summary, details) = if blocked.is_empty() {
                (
                    "success",
                    "Auto mode applied pending diffs".to_string(),
                    "Agent auto mode completed filesystem apply.".to_string(),
                )
            } else {
                (
                    "warn",
                    format!(
                        "Auto mode applied edits but held {} new file{} for review",
                        blocked.len(),
                        if blocked.len() == 1 { "" } else { "s" }
                    ),
                    format!(
                        "File creation is not permitted for this run, so these stay pending:\n{}",
                        blocked.join("\n")
                    ),
                )
            };
            self.emit_action_log(
                &app,
                level,
                "auto_apply",
                None,
                None,
                &summary,
                &details,
                Some(context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
            // 还有被拦下的新建文件时不能算 Done，否则用户看不到需要审查的内容
            if blocked.is_empty() {
                self.state_mgr
                    .set(crate::agent::state_machine::AgentState::Done);
            } else {
                self.state_mgr
                    .set(crate::agent::state_machine::AgentState::WaitingUser);
            }
        } else {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
        }
        self.emit_state(&app);

        Ok(())
    }

    /// Apply pending diffs to the workspace filesystem.
    ///
    /// 返回被权限拦下、仍保持 pending 的文件路径。这不是失败：新建文件的
    /// diff 在未授权时留给人工审查，编辑已有文件照常应用。
    pub fn apply_diffs_to_fs(&mut self) -> Result<Vec<String>, String> {
        let mut blocked: Vec<String> = Vec::new();
        let applicable: Vec<crate::agent::state_machine::FileDiff> = self
            .diffs
            .iter()
            .filter(|diff| diff.status == "pending")
            .filter(|diff| {
                if self.allow_file_create || !crate::agent::diff_apply::is_new_file_diff(diff) {
                    return true;
                }
                blocked.push(diff.file.clone());
                false
            })
            .cloned()
            .collect();

        let result = apply_pending_diffs(&applicable);

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

        Ok(blocked)
    }

    /// 拒绝一个 diff 里所有还没决定的 hunk。
    ///
    /// 和 `apply_diff` 对称：接受 `partial` / `failed`，这样"先应用一个 hunk、
    /// 再拒绝剩下的"是可行的。已 applied 的 hunk 保持不变；整体状态由
    /// `status_from_hunks` 推导，而不是硬写成 "rejected" —— 后者会让文件状态
    /// 和各 hunk 状态互相矛盾。
    pub fn reject_diff(
        &mut self,
        diff_id: &str,
    ) -> Result<crate::agent::state_machine::FileDiff, String> {
        let Some(diff) = self.diffs.iter_mut().find(|item| item.id == diff_id) else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot be rejected while status is {}",
                diff_id, diff.status
            ));
        }

        let mut rejected_any = false;
        for hunk in &mut diff.hunks {
            if matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")) {
                continue;
            }
            hunk.status = Some("rejected".to_string());
            rejected_any = true;
        }

        if !rejected_any {
            return Err(format!("Diff {} has no hunks left to reject", diff_id));
        }

        diff.status = status_from_hunks(&diff.hunks);
        let updated = diff.clone();
        self.refresh_review_state();

        Ok(updated)
    }

    /// 拒绝单个 hunk。
    pub fn reject_diff_hunk(
        &mut self,
        diff_id: &str,
        hunk_index: usize,
    ) -> Result<crate::agent::state_machine::FileDiff, String> {
        let Some(diff) = self.diffs.iter_mut().find(|item| item.id == diff_id) else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot reject hunks while status is {}",
                diff_id, diff.status
            ));
        }

        let Some(hunk) = diff.hunks.get_mut(hunk_index) else {
            return Err(format!("Hunk {} not found in diff {}", hunk_index, diff_id));
        };

        if matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")) {
            return Err(format!(
                "Hunk {} in diff {} is already {}",
                hunk_index,
                diff_id,
                hunk.status.clone().unwrap_or_default()
            ));
        }

        hunk.status = Some("rejected".to_string());
        diff.status = status_from_hunks(&diff.hunks);
        let updated = diff.clone();
        self.refresh_review_state();

        Ok(updated)
    }

    /// 应用一个 diff 里所有还没决定的 hunk。
    ///
    /// 接受 `partial` / `failed`，而不是只接受 `pending`：逐 hunk 审查过之后
    /// 整文件 Apply 仍然应该能把剩下的一次性落地。已 applied / rejected 的
    /// hunk 会被跳过，不会二次写入。
    pub fn apply_diff(
        &mut self,
        diff_id: &str,
    ) -> Result<crate::agent::state_machine::ApplyDiffsResult, String> {
        let Some(diff) = self.diffs.iter().find(|item| item.id == diff_id).cloned() else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot be applied while status is {}",
                diff_id, diff.status
            ));
        }

        let undecided: Vec<usize> = diff
            .hunks
            .iter()
            .enumerate()
            .filter(|(_, hunk)| {
                !matches!(hunk.status.as_deref(), Some("applied") | Some("rejected"))
            })
            .map(|(index, _)| index)
            .collect();

        if undecided.is_empty() {
            return Err(format!("Diff {} has no hunks left to apply", diff_id));
        }

        let synthetic = crate::agent::state_machine::FileDiff {
            hunks: undecided
                .iter()
                .map(|index| diff.hunks[*index].clone())
                .collect(),
            // 同 apply_diff_hunk：apply_pending_diffs 只处理 pending
            status: "pending".to_string(),
            ..diff.clone()
        };
        let result = apply_pending_diffs(&[synthetic]);

        if let Some(item) = self.diffs.iter_mut().find(|item| item.id == diff_id) {
            let applied = result.applied.iter().any(|entry| entry.id == item.id);
            let failed = result.failed.iter().any(|entry| entry.diff_id == item.id);
            if applied || failed {
                let status = if applied { "applied" } else { "failed" };
                for index in &undecided {
                    if let Some(hunk) = item.hunks.get_mut(*index) {
                        hunk.status = Some(status.to_string());
                    }
                }
                item.status = status_from_hunks(&item.hunks);
            }
        }
        crate::agent::diff_apply::restamp_applied_files(&mut self.diffs, &result.applied);
        self.refresh_review_state();

        Ok(result)
    }

    /// 逐 hunk 应用一个待审查 diff。
    ///
    /// 这里是纯状态操作 + 文件写入，不碰 IPC：Tauri 命令层只负责加锁、
    /// 发事件和写 action log。这样这条链路（含 baseHash 重新盖章）可以直接
    /// 用单测覆盖，而不必靠人点界面。
    pub fn apply_diff_hunk(
        &mut self,
        diff_id: &str,
        hunk_index: usize,
    ) -> Result<crate::agent::state_machine::ApplyDiffsResult, String> {
        let Some(diff) = self.diffs.iter().find(|item| item.id == diff_id).cloned() else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot apply hunks while status is {}",
                diff_id, diff.status
            ));
        }

        let Some(hunk) = diff.hunks.get(hunk_index).cloned() else {
            return Err(format!("Hunk {} not found in diff {}", hunk_index, diff_id));
        };

        if hunk.status.as_deref() == Some("applied") || hunk.status.as_deref() == Some("rejected") {
            return Err(format!(
                "Hunk {} in diff {} is already {}",
                hunk_index,
                diff_id,
                hunk.status.unwrap_or_default()
            ));
        }

        let single_hunk_diff = crate::agent::state_machine::FileDiff {
            hunks: vec![hunk],
            // 必须显式置为 pending：`apply_pending_diffs` 会跳过非 pending 的 diff，
            // 而这里继承来的状态在应用过第一个 hunk 之后是 "partial"。
            // 不重置的话第二个 hunk 会被静默跳过 —— applied 和 failed 都为空，
            // 命令返回 Ok，用户既看不到变更也看不到错误。
            status: "pending".to_string(),
            ..diff.clone()
        };
        let result = apply_pending_diffs(&[single_hunk_diff]);

        if let Some(item) = self.diffs.iter_mut().find(|item| item.id == diff_id) {
            if result.applied.iter().any(|applied| applied.id == item.id) {
                if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                    hunk.status = Some("applied".to_string());
                }
                item.status = status_from_hunks(&item.hunks);
            } else if result
                .failed
                .iter()
                .any(|failure| failure.diff_id == item.id)
            {
                if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                    hunk.status = Some("failed".to_string());
                }
                item.status = "failed".to_string();
            }
        }
        // 逐 hunk 应用会推进文件内容，后续 hunk 必须以新内容为基准，否则会被误判 stale
        crate::agent::diff_apply::restamp_applied_files(&mut self.diffs, &result.applied);
        self.refresh_review_state();

        Ok(result)
    }

    /// 还有待处理的 diff 时留在 WaitingUser，否则收尾为 Done
    pub fn refresh_review_state(&mut self) {
        let has_open_work = self.diffs.iter().any(|diff| {
            diff.status == "pending" || diff.status == "partial" || diff.status == "failed"
        });
        if has_open_work {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
        } else {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::Done);
        }
    }
}

/// 由各 hunk 的状态推导整个文件 diff 的状态
pub fn status_from_hunks(hunks: &[crate::agent::state_machine::DiffHunk]) -> String {
    let all_match = |expected: &str| {
        !hunks.is_empty()
            && hunks
                .iter()
                .all(|hunk| hunk.status.as_deref() == Some(expected))
    };
    let any_match = |expected: &str| {
        hunks
            .iter()
            .any(|hunk| hunk.status.as_deref() == Some(expected))
    };

    if all_match("applied") {
        "applied".to_string()
    } else if all_match("rejected") {
        "rejected".to_string()
    } else if any_match("failed") {
        "failed".to_string()
    } else if any_match("applied") || any_match("rejected") {
        "partial".to_string()
    } else {
        "pending".to_string()
    }
}

impl AgentOrchestrator {
    fn handle_plan_stage_response(
        &mut self,
        app: &AppHandle,
        stage: &PipelineStage,
        response: &str,
        prompt: &str,
        context_summary: String,
    ) -> usize {
        if stage.role == AgentRole::Designer {
            let artifact = executor::parse_sdd_artifact(
                response,
                prompt,
                self.last_run_id
                    .clone()
                    .or_else(|| self.current_run_id.clone()),
            );
            self.sdd_artifacts.push(artifact.clone());
            let _ = app.emit(
                "agent-sdd-ready",
                serde_json::to_value(&artifact).unwrap_or_default(),
            );
            self.emit_action_log(
                app,
                "success",
                "sdd_draft",
                Some(stage.role.to_string()),
                Some(&stage.name),
                &format!("SDD draft produced: {}", artifact.title),
                &artifact.markdown,
                Some(context_summary),
                None,
            );
            1
        } else if stage.role == AgentRole::Reviewer {
            let findings = executor::extract_review_findings(response);
            if !findings.is_empty() {
                if let Some(artifact) = self.sdd_artifacts.last_mut() {
                    artifact.review_findings.extend(findings);
                    artifact.status = "reviewed".to_string();
                    let _ = app.emit(
                        "agent-sdd-ready",
                        serde_json::to_value(artifact).unwrap_or_default(),
                    );
                }
            }
            0
        } else {
            0
        }
    }

    /// Emit the current state to the frontend.
    fn emit_state(&self, app: &AppHandle) {
        let payload = serde_json::json!({
            "state": self.state_mgr.state.to_string(),
            "mode": self.mode.to_string(),
            "ideMode": self.ide_mode.to_string(),
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

    pub fn emit_review_action_log(
        &self,
        app: &AppHandle,
        level: &str,
        phase: &str,
        summary: &str,
        details: &str,
    ) {
        self.emit_action_log(
            app,
            level,
            phase,
            None,
            Some("Diff Review"),
            summary,
            details,
            None,
            Some(self.summarize_pending_diffs()),
        );
    }

    fn pending_diff_count(&self) -> usize {
        self.diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .count()
    }

    fn summarize_pending_diffs(&self) -> String {
        let pending: Vec<_> = self
            .diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .collect();

        if pending.is_empty() {
            return "No reviewable diffs.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Reviewable diffs: {}", pending.len()));
        for diff in pending {
            lines.push(format!(
                "- {} [{}]: {} hunk{}",
                diff.file,
                diff.status,
                diff.hunks.len(),
                if diff.hunks.len() == 1 { "" } else { "s" }
            ));
            for (index, hunk) in diff.hunks.iter().enumerate() {
                lines.push(format!(
                    "  Hunk {} [{}]: -{} lines, +{} lines",
                    index + 1,
                    hunk.status.as_deref().unwrap_or("pending"),
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
            scope: None,
            execution_mode: None,
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

    /// 批量应用所有还可审查的 diff（界面上的 Apply all）。
    ///
    /// 逐文件复用 `apply_diff`，而不是把 `self.diffs` 整个交给
    /// `apply_pending_diffs`：后者只处理 `pending`，所以任何逐 hunk 审查过的
    /// 文件都会被整批跳过 —— 不报错、不落地、汇总里也看不出来。复用还顺带
    /// 保证被拒绝的 hunk 不会被批量应用重新写回去，并让 hunk 级状态和文件
    /// 状态保持一致（旧实现只改文件状态，hunk 状态留空）。
    pub fn apply_all_diffs(&mut self) -> crate::agent::state_machine::ApplyDiffsResult {
        let mut applied = Vec::new();
        let mut failed = Vec::new();

        for (id, file) in self.reviewable_diff_targets() {
            match self.apply_diff(&id) {
                Ok(result) => {
                    applied.extend(result.applied);
                    failed.extend(result.failed);
                }
                // 已经按 undecided 过滤过，走到这里说明状态机自身不一致，
                // 报出来而不是静默跳过
                Err(message) => failed.push(crate::agent::state_machine::ApplyDiffError {
                    diff_id: id,
                    file,
                    message,
                }),
            }
        }

        self.refresh_review_state();
        crate::agent::state_machine::ApplyDiffsResult { applied, failed }
    }

    /// 批量拒绝所有还可审查的 diff（界面上的 Reject all）。
    ///
    /// 返回的是本次真正改动过的 diff。旧实现返回所有 status == "rejected" 的
    /// diff，会把上一轮拒绝的也算进来，action log 的条数因此偏大。
    pub fn reject_all_diffs(&mut self) -> Vec<crate::agent::state_machine::FileDiff> {
        let mut rejected = Vec::new();
        for (id, _) in self.reviewable_diff_targets() {
            if let Ok(diff) = self.reject_diff(&id) {
                rejected.push(diff);
            }
        }
        self.refresh_review_state();
        rejected
    }

    /// 还有未决 hunk、因而值得批量处理的 diff：(id, file)
    fn reviewable_diff_targets(&self) -> Vec<(String, String)> {
        self.diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .filter(|diff| {
                diff.hunks.iter().any(|hunk| {
                    !matches!(hunk.status.as_deref(), Some("applied") | Some("rejected"))
                })
            })
            .map(|diff| (diff.id.clone(), diff.file.clone()))
            .collect()
    }

    /// 单步开始：把步骤登记为 doing，返回登记后的副本供命令层发事件
    pub fn begin_step(&mut self, step: &TaskStep, log: &str) -> TaskStep {
        self.record_step_status(step, "doing", log)
    }

    /// 更新（必要时插入）某个步骤的状态，返回登记后的副本
    pub fn record_step_status(&mut self, step: &TaskStep, status: &str, log: &str) -> TaskStep {
        let mut updated = step.clone();
        updated.status = status.to_string();
        updated.logs.push(log.to_string());
        self.upsert_step(updated.clone());
        updated
    }

    /// 单步成功：登记响应里的 diff 并收敛审查状态。
    ///
    /// 这里刻意用 `refresh_review_state` 而不是硬置 `WaitingUser`：一个只返回
    /// 文字、没有产出 diff 的步骤以前也会把界面留在"需要处理"，而审查区里
    /// 什么都没有，用户只能靠重跑脱身。
    pub fn record_step_success(
        &mut self,
        step: &TaskStep,
        response: &str,
        regenerated_from_diff_id: Option<&str>,
        regenerated_from_hunk_index: Option<usize>,
    ) -> StepRunOutcome {
        let mut step = step.clone();
        step.status = "done".to_string();
        step.logs.push(format!(
            "Single step response: {}...",
            response.chars().take(200).collect::<String>()
        ));
        self.upsert_step(step.clone());

        let parsed = executor::parse_diffs_with_diagnostics(response);
        let mut diffs = parsed.diffs;
        crate::services::agent_runtime::attach_step_provenance(
            &mut diffs,
            &step,
            regenerated_from_diff_id,
            regenerated_from_hunk_index,
        );
        // 生成时记录目标文件的内容指纹，apply 时才能识别期间发生的外部改动
        crate::agent::diff_apply::stamp_base_hashes(&mut diffs);
        let new_diffs = diffs.len();
        self.diffs.extend(diffs);
        self.refresh_review_state();

        StepRunOutcome {
            step,
            new_diffs,
            diagnostics: parsed.diagnostics,
        }
    }

    fn upsert_step(&mut self, step: TaskStep) {
        if let Some(existing) = self.steps.iter_mut().find(|item| item.id == step.id) {
            *existing = step;
        } else {
            self.steps.push(step);
        }
    }
}

/// 单步执行的结果摘要，供命令层发事件和写 action log
pub struct StepRunOutcome {
    pub step: TaskStep,
    /// 本次步骤新产出的 diff 数量（不含此前遗留的）
    pub new_diffs: usize,
    pub diagnostics: Vec<String>,
}

fn is_reviewable_diff_status(status: &str) -> bool {
    matches!(status, "pending" | "partial" | "failed")
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

fn format_context_budget_summary(
    budget: Option<&ContextBudget>,
    raw_chars: usize,
    final_chars: usize,
) -> String {
    let Some(budget) = budget else {
        return format!("Context budget: unset\nContext chars: {}", final_chars);
    };
    let estimated_input = estimated_input_tokens_from_budget(budget)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unset".to_string());
    format!(
        "Context budget: estimated input tokens={}, max context tokens={}, reserved output tokens={}\nContext chars: raw={}, final={}, trimmed={}",
        estimated_input,
        budget.max_context_tokens.map(|value| value.to_string()).unwrap_or_else(|| "unset".to_string()),
        budget.reserved_output_tokens.map(|value| value.to_string()).unwrap_or_else(|| "unset".to_string()),
        raw_chars,
        final_chars,
        final_chars < raw_chars
    )
}

fn format_context_sources(sources: &ContextSourceOptions) -> String {
    format!(
        "Context sources: projectTree={}, gitDiff={}, projectMemory={}",
        sources.include_project_tree, sources.include_git_diff, sources.include_project_memory
    )
}

fn attach_stage_provenance(
    diffs: &mut [crate::agent::state_machine::FileDiff],
    role: &str,
    stage: &str,
) {
    for diff in diffs {
        let provenance = diff.provenance.get_or_insert_with(|| DiffProvenance {
            protocol: "unknown".to_string(),
            operation: "unknown".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        });
        provenance.source_role = Some(role.to_string());
        provenance.source_stage = Some(stage.to_string());
        for (hunk_index, hunk) in diff.hunks.iter_mut().enumerate() {
            let hunk_provenance = hunk.provenance.get_or_insert_with(|| DiffHunkProvenance {
                change_index: provenance.change_index,
                hunk_index: Some(hunk_index),
                source_role: None,
                source_stage: None,
                prompt_context: None,
                rationale: provenance.rationale.clone(),
            });
            hunk_provenance.source_role = Some(role.to_string());
            hunk_provenance.source_stage = Some(stage.to_string());
            hunk_provenance.change_index = hunk_provenance.change_index.or(provenance.change_index);
            hunk_provenance.hunk_index = hunk_provenance.hunk_index.or(Some(hunk_index));
            if hunk_provenance.rationale.is_none() {
                hunk_provenance.rationale = provenance.rationale.clone();
            }
        }
    }
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
            base_hash: None,
            provenance: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: original.to_string(),
                updated: updated.to_string(),
                provenance: None,
                status: None,
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
    fn auto_apply_holds_new_files_when_file_creation_is_not_permitted() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("edit.ts", "const value = 1;\n");

        let edit = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        let create = make_diff("created.ts", "", "export const created = true;\n");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.allow_file_create = false;
        orchestrator.diffs = vec![edit, create];

        let blocked = orchestrator.apply_diffs_to_fs().unwrap();

        assert_eq!(blocked, vec!["created.ts".to_string()]);
        // 编辑已有文件照常应用
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("edit.ts")).unwrap(),
            "const value = 2;\n"
        );
        // 新建文件既没写盘，也没被标记失败——它留待人工审查
        assert_eq!(orchestrator.diffs[1].status, "pending");
        assert!(!env.root.join("created.ts").exists());
    }

    #[test]
    fn auto_apply_creates_new_files_once_permitted() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();

        let create = make_diff("created.ts", "", "export const created = true;\n");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.allow_file_create = true;
        orchestrator.diffs = vec![create];

        let blocked = orchestrator.apply_diffs_to_fs().unwrap();

        assert!(blocked.is_empty());
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("created.ts")).unwrap(),
            "export const created = true;\n"
        );
    }

    /// 今天靠人点界面验证的那条路径，现在是自动测试：逐 hunk 应用会推进文件
    /// 内容，第二个 hunk 必须仍能应用 —— 如果 restamp 没生效，它会因为
    /// baseHash 不匹配被误判为 stale。
    #[test]
    fn applying_hunks_one_by_one_keeps_the_rest_appliable() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        let first = orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(first.applied.len(), 1, "failed: {:?}", first.failed);
        assert_eq!(orchestrator.diffs[0].status, "partial");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );

        let second = orchestrator.apply_diff_hunk(&diff_id, 1).unwrap();
        assert_eq!(
            second.applied.len(),
            1,
            "second hunk was rejected: {:?}",
            second.failed
        );

        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        // 全部 hunk 落地后审查结束
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn apply_diff_hunk_rejects_unknown_ids_and_repeated_hunks() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("edit.ts", "const value = 1;\n");

        let diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        assert!(orchestrator
            .apply_diff_hunk("missing", 0)
            .unwrap_err()
            .contains("Diff not found"));
        assert!(orchestrator
            .apply_diff_hunk(&diff_id, 9)
            .unwrap_err()
            .contains("Hunk 9 not found"));

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        // 单 hunk 全部应用后整个 diff 变成 applied，此时不允许再次应用
        assert!(orchestrator
            .apply_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("cannot apply hunks while status is applied"));
    }

    /// 逐 hunk 审查过之后，整文件 Apply 仍然要能把剩下的落地。
    /// 旧实现只接受 `pending`，所以点过任意一个 hunk 之后整文件按钮就报
    /// "not pending"，用户只能一个个点完剩下的。
    #[test]
    fn apply_diff_finishes_a_partially_reviewed_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let rest = orchestrator.apply_diff(&diff_id).unwrap();

        assert_eq!(rest.applied.len(), 1, "failed: {:?}", rest.failed);
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        // 没有剩余 hunk 时再点应该明确报错，而不是静默成功
        assert!(orchestrator
            .apply_diff(&diff_id)
            .unwrap_err()
            .contains("cannot be applied while status is applied"));
    }

    #[test]
    fn apply_diff_skips_hunks_that_were_already_rejected() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks[0].status = Some("rejected".to_string());
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        diff.status = "partial".to_string();
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff(&diff_id).unwrap();

        // 被拒绝的 hunk 不能被整文件 Apply 重新写回去
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 1;\nconst second = 2;\n"
        );
        assert_eq!(
            orchestrator.diffs[0].hunks[0].status.as_deref(),
            Some("rejected")
        );
        assert_eq!(orchestrator.diffs[0].status, "partial");
    }

    /// 拒绝路径和应用路径共享同一套 hunk 状态收敛逻辑，这里覆盖它们交叉的场景：
    /// 先应用一个 hunk，再整文件 Reject 剩下的。旧实现要求 status == "pending"，
    /// 所以这一步会直接报 "not pending"。
    #[test]
    fn reject_diff_finishes_a_partially_applied_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let rejected = orchestrator.reject_diff(&diff_id).unwrap();

        // 已应用的 hunk 不能被整文件 Reject 反写成 rejected
        assert_eq!(rejected.hunks[0].status.as_deref(), Some("applied"));
        assert_eq!(rejected.hunks[1].status.as_deref(), Some("rejected"));
        // 整体状态由 hunk 推导：applied + rejected 混合 => partial，而不是硬写的 "rejected"
        assert_eq!(rejected.status, "partial");
        // 拒绝不写盘：第一个 hunk 的改动留着，第二个原样不动
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 1;\n"
        );
        // 没有可拒绝的 hunk 时必须明确报错，而不是静默成功
        assert!(orchestrator
            .reject_diff(&diff_id)
            .unwrap_err()
            .contains("no hunks left to reject"));
    }

    #[test]
    fn reject_diff_hunk_converges_status_and_refuses_repeats() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];

        let first = orchestrator.reject_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(first.status, "partial");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );

        let second = orchestrator.reject_diff_hunk(&diff_id, 1).unwrap();
        assert_eq!(second.status, "rejected");
        // 全部决定完毕后不该继续挂在 WaitingUser
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );

        assert!(orchestrator
            .reject_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("while status is rejected"));
        assert!(orchestrator
            .reject_diff_hunk("missing", 0)
            .unwrap_err()
            .contains("Diff not found"));
    }

    #[test]
    fn reject_diff_hunk_keeps_an_applied_hunk_untouched() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();

        assert!(orchestrator
            .reject_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("is already applied"));
        assert_eq!(
            orchestrator.diffs[0].hunks[0].status.as_deref(),
            Some("applied")
        );
    }

    /// Apply all 以前直接把 `self.diffs` 交给 `apply_pending_diffs`，而后者只处理
    /// `pending` —— 逐 hunk 审查过的文件会被整批静默跳过。
    #[test]
    fn apply_all_diffs_includes_partially_reviewed_files() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");
        env.write_file("other.ts", "const other = 1;\n");

        let mut multi = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        multi.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let multi_id = multi.id.clone();
        let other = make_diff("other.ts", "const other = 1;", "const other = 2;");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![multi, other];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&multi_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let result = orchestrator.apply_all_diffs();

        assert!(result.failed.is_empty(), "failed: {:?}", result.failed);
        // partial 的文件也被收尾，而不是被跳过
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("other.ts")).unwrap(),
            "const other = 2;\n"
        );
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(orchestrator.diffs[1].status, "applied");
        // hunk 级状态也要跟上，否则界面上的逐 hunk 标记会是空的
        assert!(orchestrator.diffs[0]
            .hunks
            .iter()
            .all(|hunk| hunk.status.as_deref() == Some("applied")));
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn apply_all_diffs_does_not_resurrect_rejected_hunks() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.reject_diff_hunk(&diff_id, 0).unwrap();
        let result = orchestrator.apply_all_diffs();

        assert!(result.failed.is_empty(), "failed: {:?}", result.failed);
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 1;\nconst second = 2;\n"
        );
        assert_eq!(orchestrator.diffs[0].status, "partial");
    }

    /// Reject all 以前只处理 `pending`，并且把所有历史 rejected diff 都算作本次
    /// 结果 —— action log 因此虚报条数。
    #[test]
    fn reject_all_diffs_reports_only_what_it_changed() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("a.ts", "const a = 1;\n");
        env.write_file("b.ts", "const b = 1;\n");

        let a = make_diff("a.ts", "const a = 1;", "const a = 2;");
        let a_id = a.id.clone();
        let b = make_diff("b.ts", "const b = 1;", "const b = 2;");
        let b_id = b.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![a, b];

        let first = orchestrator.reject_all_diffs();
        assert_eq!(first.len(), 2);

        // 第二轮新来一个 diff：只应报告这一个，而不是连上一轮的两个
        orchestrator
            .diffs
            .push(make_diff("c.ts", "const c = 1;", "const c = 2;"));
        let second = orchestrator.reject_all_diffs();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].file, "c.ts");

        assert!(orchestrator
            .diffs
            .iter()
            .all(|diff| diff.status == "rejected"));
        // 拒绝不写盘
        assert_eq!(
            std::fs::read_to_string(env.root.join("a.ts")).unwrap(),
            "const a = 1;\n"
        );
        assert!(orchestrator.diffs.iter().any(|diff| diff.id == a_id));
        assert!(orchestrator.diffs.iter().any(|diff| diff.id == b_id));
    }

    fn make_step(id: &str) -> TaskStep {
        TaskStep {
            id: id.to_string(),
            title: format!("Step {}", id),
            step_type: "code".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: None,
            execution_mode: None,
        }
    }

    /// 一个只返回文字、没有产出 diff 的步骤以前会把状态硬置成 WaitingUser，
    /// 界面显示"需要处理"但审查区是空的，用户只能靠重跑脱身。
    #[test]
    fn a_step_without_diffs_does_not_park_the_ui_in_waiting_user() {
        let _guard = workspace::env_test_guard();
        let _env = TestEnv::new();

        let step = make_step("s1");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.begin_step(&step, "started");
        assert_eq!(orchestrator.steps[0].status, "doing");

        let outcome =
            orchestrator.record_step_success(&step, "s1 done, nothing to change.", None, None);

        assert_eq!(outcome.new_diffs, 0);
        assert_eq!(outcome.step.status, "done");
        assert_eq!(orchestrator.steps.len(), 1, "步骤应被就地更新而不是追加");
        assert_eq!(orchestrator.steps[0].status, "done");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn a_step_with_diffs_stamps_base_hashes_and_waits_for_review() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/app.ts", "const value = 1;\n");

        let step = make_step("s1");
        let mut orchestrator = AgentOrchestrator::new();
        // 上一轮遗留的一个 diff：new_diffs 只应统计本次新增的
        orchestrator
            .diffs
            .push(make_diff("old.ts", "const old = 1;", "const old = 2;"));

        let response = "```diff:src/app.ts\n<<<<<<< ORIGINAL\nconst value = 1;\n=======\nconst value = 2;\n>>>>>>> UPDATED\n```";
        let outcome = orchestrator.record_step_success(&step, response, None, None);

        assert_eq!(outcome.new_diffs, 1);
        assert_eq!(orchestrator.diffs.len(), 2);
        // baseHash 必须在生成时盖章，否则 apply 时无法识别期间的外部改动
        assert!(orchestrator.diffs[1].base_hash.is_some());
        assert_eq!(
            orchestrator.diffs[1]
                .provenance
                .as_ref()
                .unwrap()
                .source_stage,
            Some("Step s1".to_string())
        );
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );
    }

    /// 单次运行上限只要中途暂停一次就归零重算的话就形同虚设，所以
    /// `begin_run` 不能清掉记账器，`resumed_usage_meter` 必须返回同一个实例。
    #[test]
    fn token_accounting_survives_a_pause_and_resume() {
        let meter = std::sync::Arc::new(crate::services::llm_client::RunUsageMeter::new(Some(100)));
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.start_usage_accounting(meter.clone());

        meter.record_usage(Some(&crate::services::llm_client::LlmUsage {
            prompt_tokens: Some(60),
            completion_tokens: Some(10),
            total_tokens: None,
        }));

        // 恢复时 run id 会重新写入，但额度必须接着算
        orchestrator.begin_run(Some("run-1".to_string()));
        let resumed = orchestrator
            .resumed_usage_meter()
            .expect("paused run should hand its meter back");

        assert_eq!(resumed.snapshot().total_tokens, 70);
        resumed.record_usage(Some(&crate::services::llm_client::LlmUsage {
            prompt_tokens: Some(40),
            completion_tokens: None,
            total_tokens: None,
        }));
        // 续跑的消耗算在同一个额度里，因此这里已经越线
        assert!(resumed.check_budget().is_err());
        assert_eq!(meter.snapshot().total_tokens, 110);
    }

    /// 每次运行原本都是冷启动，跟进一句"再处理下错误分支"读不到上一轮做了什么。
    #[test]
    fn conversation_turns_carry_forward_and_stay_bounded() {
        let mut orchestrator = AgentOrchestrator::new();
        assert!(orchestrator.conversation_digest().is_none());

        orchestrator
            .diffs
            .push(make_diff("src/app.ts", "const a = 1;", "const a = 2;"));
        orchestrator.record_conversation_turn("rename the value");

        let digest = orchestrator.conversation_digest().expect("digest");
        assert!(digest.contains("rename the value"), "{}", digest);
        // 结果里要说清上一轮留下了什么，否则"接着上一句"仍然无从下手
        assert!(digest.contains("src/app.ts"), "{}", digest);
        assert!(digest.contains("awaiting review"), "{}", digest);

        // 只保留末尾若干轮：早期轮次对"接着上一句"没帮助，却一直占预算
        for index in 0..MAX_CONVERSATION_TURNS * 2 {
            orchestrator.record_conversation_turn(&format!("turn {}", index));
        }
        assert_eq!(orchestrator.conversation.len(), MAX_CONVERSATION_TURNS);
        let digest = orchestrator.conversation_digest().expect("digest");
        assert!(!digest.contains("rename the value"), "{}", digest);

        orchestrator.clear_conversation();
        assert!(orchestrator.conversation_digest().is_none());
    }

    #[test]
    fn a_run_without_changes_is_recorded_as_such() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.record_conversation_turn("explain the build pipeline");

        let digest = orchestrator.conversation_digest().expect("digest");
        // 不能记成"改了文件"，否则下一轮模型会以为已经动过代码
        assert!(digest.contains("no file changes produced"), "{}", digest);
    }

    #[test]
    fn summarize_pending_diffs_includes_actual_reviewable_diff_context() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![
            make_diff("src/app.ts", "const oldValue = 1;", "const newValue = 2;"),
            FileDiff {
                status: "partial".to_string(),
                ..make_diff("src/partial.ts", "partial(false)", "partial(true)")
            },
            FileDiff {
                status: "applied".to_string(),
                ..make_diff("src/done.ts", "done()", "done(true)")
            },
        ];

        let summary = orchestrator.summarize_pending_diffs();

        assert!(summary.contains("Reviewable diffs: 2"));
        assert!(summary.contains("src/app.ts"));
        assert!(summary.contains("src/partial.ts [partial]"));
        assert!(summary.contains("Original excerpt: const oldValue = 1;"));
        assert!(summary.contains("Updated excerpt: const newValue = 2;"));
        assert!(!summary.contains("src/done.ts"));
    }

    #[test]
    fn attach_stage_provenance_records_role_and_stage() {
        let mut diffs = vec![make_diff("src/app.ts", "old", "new")];

        attach_stage_provenance(&mut diffs, "coder", "Implement");

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("coder"));
        assert_eq!(provenance.source_stage.as_deref(), Some("Implement"));
    }

    #[test]
    fn run_id_tracks_current_and_last_run() {
        let mut orchestrator = AgentOrchestrator::new();

        orchestrator.begin_run(Some("run-1".to_string()));

        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-1"));
        assert_eq!(orchestrator.last_run_id.as_deref(), Some("run-1"));

        orchestrator.finish_run();

        assert_eq!(orchestrator.current_run_id, None);
        assert_eq!(orchestrator.last_run_id.as_deref(), Some("run-1"));
    }

    #[test]
    fn paused_pipeline_snapshot_can_be_stored_for_resume() {
        let mut orchestrator = AgentOrchestrator::new();
        let pipeline = crate::agent::multi_agent::default_pipeline();

        orchestrator.paused_run = Some(PausedPipelineRun {
            prompt: "Fix issue".to_string(),
            context: "context".to_string(),
            context_summary: "summary".to_string(),
            stage_outputs: vec!["Planner output".to_string()],
            pipeline: pipeline.clone(),
            stage_index: 1,
            ide_mode: IdeMode::Code,
        });

        let paused = orchestrator.paused_run.as_ref().expect("paused run");
        assert_eq!(paused.stage_index, 1);
        assert_eq!(paused.pipeline.len(), pipeline.len());
        assert_eq!(paused.stage_outputs[0], "Planner output");
    }

    #[test]
    fn format_context_sources_records_workspace_source_flags() {
        let summary = format_context_sources(&crate::services::context::ContextSourceOptions {
            include_project_tree: true,
            include_git_diff: false,
            include_project_memory: true,
        });

        assert_eq!(
            summary,
            "Context sources: projectTree=true, gitDiff=false, projectMemory=true"
        );
    }
}
