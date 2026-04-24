use crate::agent::state_machine::{AgentMode, AgentStateManager, TaskStep};
use crate::agent::planner;
use crate::agent::executor;
use crate::services::llm_client::LlmClient;
use crate::services::context::AgentContext;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::mpsc;

/// Agent 编排器 —— 主流程控制器
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

    /// 运行完整的 Agent 流程：
    /// prompt → LLM plan → execute steps → generate diffs → await user
    pub async fn run(
        &mut self,
        prompt: String,
        context: AgentContext,
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
        let ctx_str = context.to_prompt_context();
        let (tx, mut rx) = mpsc::channel::<String>(32);

        // 发射流式 token 到前端
        let app_clone = app.clone();
        tokio::spawn(async move {
            while let Some(token) = rx.recv().await {
                let _ = app_clone.emit("agent-stream-token", token);
            }
        });

        let (steps, _full_response) =
            planner::plan_task(llm, &prompt, &ctx_str, tx).await?;

        self.steps = steps;

        // 3. Transition to Planning
        let _ = self.state_mgr.transition(&AgentEvent::PlanReady(self.steps.clone()));
        self.emit_state(&app);
        let _ = app.emit("agent-plan-ready", serde_json::to_value(&self.steps).unwrap_or_default());

        // 4. Execute each step
        let steps_len = self.steps.len();
        for i in 0..steps_len {
            let step_title = self.steps[i].title.clone();
            let step_type = self.steps[i].step_type.clone();

            // 更新步骤状态为 doing
            self.steps[i].status = "doing".to_string();
            let _ = app.emit(
                "agent-step-update",
                serde_json::to_value(&self.steps[i]).unwrap_or_default(),
            );
            let _ = self.state_mgr.transition(&AgentEvent::StepStart(step_title.clone()));
            self.emit_state(&app);

            // 流式调用 LLM 执行步骤
            let (tx2, mut rx2) = mpsc::channel::<String>(32);
            let app_clone2 = app.clone();
            tokio::spawn(async move {
                while let Some(token) = rx2.recv().await {
                    let _ = app_clone2.emit("agent-stream-token", token);
                }
            });

            let step_context = format!(
                "Task: {}\nStep: {}\nType: {}\nContext: {}",
                prompt, step_title, step_type, ctx_str
            );

            match executor::execute_step(llm, &step_title, &step_context, tx2).await {
                Ok(response) => {
                    self.steps[i].status = "done".to_string();
                    self.steps[i]
                        .logs
                        .push(format!("LLM response: {}...", &response[..response.len().min(200)]));

                    // 解析 diffs
                    let step_diffs = executor::parse_diffs(&response);
                    self.diffs.extend(step_diffs);
                }
                Err(e) => {
                    self.steps[i].status = "error".to_string();
                    self.steps[i].logs.push(format!("Error: {}", e));
                }
            }

            let _ = app.emit(
                "agent-step-update",
                serde_json::to_value(&self.steps[i]).unwrap_or_default(),
            );
            let _ = self.state_mgr.transition(&AgentEvent::StepDone(step_title.clone()));
            self.emit_state(&app);

            // 非 Auto 模式下，完成一步后等待确认
            if self.mode != AgentMode::Auto {
                self.state_mgr.set(crate::agent::state_machine::AgentState::WaitingUser);
                self.emit_state(&app);
                // 发出 diff 事件
                if !self.diffs.is_empty() {
                    let _ = app.emit(
                        "agent-diff-ready",
                        serde_json::to_value(&self.diffs).unwrap_or_default(),
                    );
                    let _ = self
                        .state_mgr
                        .transition(&AgentEvent::DiffReady(self.diffs.clone()));
                }
                return Ok(());
            }

            // 步骤间短暂延迟
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }

        // 5. Reviewing → WaitingUser
        if !self.diffs.is_empty() {
            let _ = app.emit(
                "agent-diff-ready",
                serde_json::to_value(&self.diffs).unwrap_or_default(),
            );
        }
        let _ = self.state_mgr.transition(&AgentEvent::DiffReady(self.diffs.clone()));
        self.state_mgr.set(crate::agent::state_machine::AgentState::WaitingUser);
        self.emit_state(&app);

        Ok(())
    }

    /// 发出当前状态到前端
    fn emit_state(&self, app: &AppHandle) {
        let payload = serde_json::json!({
            "state": self.state_mgr.state.to_string(),
            "mode": self.mode.to_string(),
        });
        let _ = app.emit("agent-state-changed", payload);
    }

    /// 应用所有 pending diff
    pub fn apply_diffs(&mut self) {
        for diff in &mut self.diffs {
            if diff.status == "pending" {
                diff.status = "applied".to_string();
            }
        }
    }

    /// 拒绝所有 pending diff
    pub fn reject_diffs(&mut self) {
        for diff in &mut self.diffs {
            if diff.status == "pending" {
                diff.status = "rejected".to_string();
            }
        }
    }
}
