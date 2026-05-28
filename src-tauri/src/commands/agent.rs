use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::executor;
use crate::agent::multi_agent::{default_pipeline, AgentRole, PipelineStage};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::agent::state_machine::{
    AgentMode, AgentState, ApplyDiffsResult, FileDiff, IdeMode, SddArtifact, TaskStep,
};
use crate::services::agent_runtime;
use crate::services::context::{
    AgentContext, ContextBudget, ContextBuildOptions, ContextCompressionMode,
    ContextEstimateResponse, ContextSourceOptions,
};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::llm_profiles::{
    self, LlmProfileResponse, LlmProfilesConfig, LlmProfilesResponse, SaveLlmProfileRequest,
};
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Emitter;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

/// Global Agent state. Uses tokio::sync::Mutex for async orchestration.

pub struct AgentGlobalState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub llm_profiles: Arc<std::sync::Mutex<LlmProfilesConfig>>,
    pub active_role: Arc<std::sync::Mutex<AgentRole>>,
    pub pipeline_stages: Arc<std::sync::Mutex<Vec<PipelineStage>>>,
    pub context_compression: Arc<std::sync::Mutex<ContextCompressionMode>>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl AgentGlobalState {
    pub fn new() -> Self {
        let profiles_config = llm_profiles::load_or_default_config();
        let context_compression = profiles_config.context_compression.clone();

        Self {
            orchestrator: Arc::new(Mutex::new(AgentOrchestrator::new())),
            llm_profiles: Arc::new(std::sync::Mutex::new(profiles_config)),
            active_role: Arc::new(std::sync::Mutex::new(AgentRole::Coder)),
            pipeline_stages: Arc::new(std::sync::Mutex::new(default_pipeline())),
            context_compression: Arc::new(std::sync::Mutex::new(context_compression)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a cloned LLM client.
    pub fn get_llm_client(&self, profile_id: Option<&str>) -> Result<LlmClient, String> {
        Ok(LlmClient::new(self.get_llm_config(profile_id)?))
    }

    pub fn get_llm_config(&self, profile_id: Option<&str>) -> Result<LlmConfig, String> {
        let profiles = self.llm_profiles.lock().map_err(|e| e.to_string())?;
        llm_profiles::resolve_llm_config(&profiles, profile_id)
    }

    pub fn get_context_budget(&self, profile_id: Option<&str>) -> Option<ContextBudget> {
        let profiles = self.llm_profiles.lock().ok()?;
        llm_profiles::context_budget(&profiles, profile_id)
    }
}

/// Agent status response DTO.
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
    #[serde(rename = "ideMode")]
    pub ide_mode: String,
    pub context_files: Vec<String>,
    #[serde(rename = "currentRunId")]
    pub current_run_id: Option<String>,
    #[serde(rename = "lastRunId")]
    pub last_run_id: Option<String>,
}

/// Send-prompt request DTO.
#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
    #[serde(rename = "ideMode")]
    pub ide_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveSddArtifactRequest {
    pub artifact: SddArtifact,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize)]
pub struct SavedSddArtifactResponse {
    pub path: String,
    pub artifact: SddArtifact,
}

#[derive(Debug, Deserialize)]
pub struct EstimateContextRequest {
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
}

#[derive(Debug, Deserialize)]
pub struct RunAgentStepRequest {
    pub step: TaskStep,
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
    #[serde(rename = "extraPrompt")]
    pub extra_prompt: Option<String>,
    #[serde(rename = "regeneratedFromDiffId")]
    pub regenerated_from_diff_id: Option<String>,
    #[serde(rename = "regeneratedFromHunkIndex")]
    pub regenerated_from_hunk_index: Option<usize>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
}

/// Get the current Agent state.
#[tauri::command]
pub async fn get_agent_state(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<AgentStatus, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(AgentStatus {
        state: orch.state_mgr.state.to_string(),
        mode: orch.mode.to_string(),
        ide_mode: orch.ide_mode.to_string(),
        context_files: Vec::new(),
        current_run_id: orch.current_run_id.clone(),
        last_run_id: orch.last_run_id.clone(),
    })
}

#[tauri::command]
pub async fn estimate_agent_context(
    request: EstimateContextRequest,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ContextEstimateResponse, String> {
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());
    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
    );
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    context.enrich_from_workspace_with_sources(&context_sources);
    let compression =
        resolve_context_compression(&agent_state, request.context_compression.as_deref())?;

    Ok(context.estimate_prompt_context(&ContextBuildOptions::new(compression, context_budget)))
}

/// Send a prompt to the Agent.
#[tauri::command]
pub async fn send_agent_prompt(
    request: SendPromptRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client(request.profile_id.as_deref())?;
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());

    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
    );
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    context.enrich_from_workspace_with_sources(&context_sources);

    // The async mutex can be held safely while the orchestrator runs.
    let compression =
        resolve_context_compression(&agent_state, request.context_compression.as_deref())?;
    let pipeline = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let ide_mode = request
        .ide_mode
        .as_deref()
        .map(IdeMode::from_str)
        .transpose()?
        .unwrap_or(IdeMode::Code);
    agent_state.cancel_flag.store(false, Ordering::SeqCst);
    let cancel_flag = agent_state.cancel_flag.clone();
    let mut orch = agent_state.orchestrator.lock().await;
    orch.begin_run(request.run_id.clone());
    match orch
        .run(
            request.prompt,
            context,
            compression,
            context_budget,
            context_sources,
            pipeline,
            ide_mode,
            cancel_flag,
            &llm,
            app_handle.clone(),
        )
        .await
    {
        Ok(()) => {
            orch.finish_run();
        }
        Err(err) if is_cancelled_error(&err) => {
            orch.finish_run();
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({
                    "state": orch.state_mgr.state.to_string(),
                    "mode": orch.mode.to_string(),
                    "ideMode": ide_mode.to_string(),
                    "currentRunId": orch.current_run_id,
                    "lastRunId": orch.last_run_id,
                }),
            );
            return Ok("Agent task cancelled".to_string());
        }
        Err(err) => {
            orch.finish_run();
            return Err(err);
        }
    }

    Ok("Agent task completed".to_string())
}

/// Stop the current Agent task.
#[tauri::command]
pub async fn stop_agent(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    agent_state.cancel_flag.store(true, Ordering::SeqCst);
    let mut orch = agent_state.orchestrator.lock().await;
    orch.finish_run();
    orch.state_mgr.set(AgentState::Idle);
    orch.ide_mode = IdeMode::Code;
    orch.steps.clear();
    orch.diffs.clear();
    orch.sdd_artifacts.clear();
    Ok("Agent stopped".to_string())
}

#[tauri::command]
pub async fn update_agent_step(
    step: TaskStep,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<TaskStep, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(existing) = orch.steps.iter_mut().find(|item| item.id == step.id) else {
        return Err(format!("Step not found: {}", step.id));
    };
    *existing = step.clone();
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_update",
        &format!("Updated step {}", step.title),
        &format!(
            "Step: {}\nScope: {}\nExecution mode: {}",
            step.title,
            step.scope.as_deref().unwrap_or("default"),
            step.execution_mode.as_deref().unwrap_or("default")
        ),
    );
    Ok(step)
}

#[tauri::command]
pub async fn update_agent_steps(
    steps: Vec<TaskStep>,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.steps = steps.clone();
    let _ = app_handle.emit(
        "agent-plan-ready",
        serde_json::to_value(&steps).unwrap_or_default(),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_update",
        "Updated Agent plan step order",
        &format!(
            "Steps:\n{}",
            steps
                .iter()
                .enumerate()
                .map(|(index, step)| format!("{}. {}", index + 1, step.title))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    Ok(steps)
}

#[tauri::command]
pub async fn skip_agent_step(
    step_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<TaskStep, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(step) = orch.steps.iter_mut().find(|item| item.id == step_id) else {
        return Err(format!("Step not found: {}", step_id));
    };
    step.status = "skipped".to_string();
    step.logs.push("Skipped by user".to_string());
    let updated = step.clone();
    let _ = app_handle.emit(
        "agent-step-update",
        serde_json::to_value(&updated).unwrap_or_default(),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_skip",
        &format!("Skipped step {}", updated.title),
        &format!("Step id: {}", updated.id),
    );
    Ok(updated)
}

#[tauri::command]
pub async fn run_agent_step(
    request: RunAgentStepRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client(request.profile_id.as_deref())?;
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());
    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
    );
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    context.enrich_from_workspace_with_sources(&context_sources);
    let compression =
        resolve_context_compression(&agent_state, request.context_compression.as_deref())?;
    let ctx_str = context.to_prompt_context_with_options(&ContextBuildOptions::new(
        compression.clone(),
        context_budget,
    ));
    let mut step = request.step;
    let step_prompt =
        agent_runtime::format_single_step_prompt(&step, request.extra_prompt.as_deref());

    agent_state.cancel_flag.store(false, Ordering::SeqCst);
    let cancel_flag = agent_state.cancel_flag.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            let _ = app_clone.emit("agent-stream-token", token);
        }
    });

    {
        let mut orch = agent_state.orchestrator.lock().await;
        orch.begin_run(request.run_id.clone());
        upsert_step_status(
            &mut orch.steps,
            &step,
            "doing",
            "Single step execution started",
        );
        let _ = app_handle.emit(
            "agent-step-update",
            serde_json::to_value(
                &orch
                    .steps
                    .iter()
                    .find(|item| item.id == step.id)
                    .unwrap_or(&step),
            )
            .unwrap_or_default(),
        );
        orch.emit_review_action_log(
            &app_handle,
            "info",
            "plan_run_step",
            &format!("Running step {}", step.title),
            &format!(
                "Scope: {}\nExecution mode: {}\nContext mode: {}",
                step.scope.as_deref().unwrap_or("workspace"),
                step.execution_mode.as_deref().unwrap_or("diff"),
                compression
            ),
        );
    }

    let response =
        crate::agent::executor::execute_step(&llm, &step_prompt, &ctx_str, cancel_flag, tx).await;
    let mut orch = agent_state.orchestrator.lock().await;
    match response {
        Ok(response) => {
            step.status = "done".to_string();
            step.logs.push(format!(
                "Single step response: {}...",
                response.chars().take(200).collect::<String>()
            ));
            upsert_step(&mut orch.steps, step.clone());

            let parsed = crate::agent::executor::parse_diffs_with_diagnostics(&response);
            let mut diffs = parsed.diffs;
            agent_runtime::attach_step_provenance(
                &mut diffs,
                &step,
                request.regenerated_from_diff_id.as_deref(),
                request.regenerated_from_hunk_index,
            );
            orch.diffs.extend(diffs);
            if !parsed.diagnostics.is_empty() {
                orch.emit_review_action_log(
                    &app_handle,
                    "warn",
                    "agent_changes_validation",
                    "Agent changes validation reported issues",
                    &parsed.diagnostics.join("\n"),
                );
            }
            let _ = app_handle.emit(
                "agent-step-update",
                serde_json::to_value(&step).unwrap_or_default(),
            );
            let _ = app_handle.emit(
                "agent-diff-ready",
                serde_json::to_value(&orch.diffs).unwrap_or_default(),
            );
            orch.state_mgr.set(AgentState::WaitingUser);
            orch.finish_run();
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({
                    "state": orch.state_mgr.state.to_string(),
                    "mode": orch.mode.to_string(),
                    "currentRunId": orch.current_run_id,
                    "lastRunId": orch.last_run_id,
                }),
            );
            orch.emit_review_action_log(
                &app_handle,
                "success",
                "plan_run_step",
                &format!(
                    "Step completed with {} pending diff{}",
                    orch.diffs
                        .iter()
                        .filter(|diff| matches!(
                            diff.status.as_str(),
                            "pending" | "partial" | "failed"
                        ))
                        .count(),
                    if orch
                        .diffs
                        .iter()
                        .filter(|diff| matches!(
                            diff.status.as_str(),
                            "pending" | "partial" | "failed"
                        ))
                        .count()
                        == 1
                    {
                        ""
                    } else {
                        "s"
                    }
                ),
                &response,
            );
            Ok("Agent step completed".to_string())
        }
        Err(err) if is_cancelled_error(&err) => {
            orch.finish_run();
            upsert_step_status(
                &mut orch.steps,
                &step,
                "todo",
                "Single step execution cancelled",
            );
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({
                    "state": orch.state_mgr.state.to_string(),
                    "mode": orch.mode.to_string(),
                    "currentRunId": orch.current_run_id,
                    "lastRunId": orch.last_run_id,
                }),
            );
            Ok("Agent task cancelled".to_string())
        }
        Err(err) => {
            orch.finish_run();
            upsert_step_status(&mut orch.steps, &step, "error", &format!("Error: {}", err));
            let _ = app_handle.emit(
                "agent-step-update",
                serde_json::to_value(
                    orch.steps
                        .iter()
                        .find(|item| item.id == step.id)
                        .unwrap_or(&step),
                )
                .unwrap_or_default(),
            );
            orch.state_mgr.set(AgentState::Error(err.clone()));
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({
                    "state": orch.state_mgr.state.to_string(),
                    "mode": orch.mode.to_string(),
                    "currentRunId": orch.current_run_id,
                    "lastRunId": orch.last_run_id,
                }),
            );
            orch.emit_review_action_log(
                &app_handle,
                "error",
                "plan_run_step",
                &format!("Step failed {}", step.title),
                &err,
            );
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn continue_agent_pipeline(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client(None)?;
    agent_state.cancel_flag.store(false, Ordering::SeqCst);
    let cancel_flag = agent_state.cancel_flag.clone();
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(paused) = orch.paused_run.take() else {
        return Err("No paused Agent pipeline to continue.".to_string());
    };
    let run_id = orch.last_run_id.clone();
    orch.begin_run(run_id);
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "pipeline_continue",
        "Continuing paused Agent pipeline",
        &format!("Continuing from stage {}", paused.stage_index + 1),
    );
    match orch
        .continue_pipeline_from(
            paused.prompt,
            paused.context,
            paused.context_summary,
            paused.pipeline,
            paused.stage_outputs,
            paused.stage_index,
            true,
            paused.ide_mode,
            cancel_flag,
            &llm,
            app_handle.clone(),
        )
        .await
    {
        Ok(()) => {
            orch.finish_run();
            Ok("Agent pipeline continued".to_string())
        }
        Err(err) if is_cancelled_error(&err) => {
            orch.finish_run();
            orch.state_mgr.set(AgentState::Idle);
            let ide_mode = orch.ide_mode;
            let _ = app_handle.emit(
                "agent-state-changed",
                serde_json::json!({
                    "state": orch.state_mgr.state.to_string(),
                    "mode": orch.mode.to_string(),
                    "ideMode": ide_mode.to_string(),
                    "currentRunId": orch.current_run_id,
                    "lastRunId": orch.last_run_id,
                }),
            );
            Ok("Agent task cancelled".to_string())
        }
        Err(err) => {
            orch.finish_run();
            Err(err)
        }
    }
}

/// Set the Agent mode.
#[tauri::command]
pub async fn set_agent_mode(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.mode = match mode.as_str() {
        "suggest" => AgentMode::Suggest,
        "edit" => AgentMode::Edit,
        "auto" => AgentMode::Auto,
        _ => return Err(format!("Invalid mode: {}", mode)),
    };
    Ok(())
}

/// Apply all pending diffs to the filesystem.
#[tauri::command]
pub async fn apply_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let result = apply_pending_diffs(&orch.diffs);
    let applied = result.applied.clone();
    let failed = result.failed.clone();

    for diff in &mut orch.diffs {
        if diff.status != "pending" {
            continue;
        }
        if applied.iter().any(|item| item.id == diff.id) {
            diff.status = "applied".to_string();
        } else if failed.iter().any(|item| item.diff_id == diff.id) {
            diff.status = "failed".to_string();
        }
    }

    if failed.is_empty() {
        orch.state_mgr
            .transition(&crate::agent::state_machine::AgentEvent::UserApply);
    } else {
        orch.state_mgr.set(AgentState::WaitingUser);
    }
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );
    orch.emit_review_action_log(
        &app_handle,
        if failed.is_empty() { "success" } else { "warn" },
        "diff_apply",
        &format!(
            "Apply all diffs: {} applied, {} failed",
            applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

/// Apply one pending diff to the filesystem.
#[tauri::command]
pub async fn apply_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch.diffs.iter().find(|item| item.id == diff_id).cloned() else {
        return Err(format!("Diff not found: {}", diff_id));
    };

    if diff.status != "pending" {
        return Err(format!(
            "Diff {} is not pending; current status is {}",
            diff_id, diff.status
        ));
    }

    let result = apply_pending_diffs(&[diff]);
    let failed = result.failed.clone();

    for item in &mut orch.diffs {
        if item.id != diff_id {
            continue;
        }
        if result.applied.iter().any(|applied| applied.id == item.id) {
            item.status = "applied".to_string();
        } else if failed.iter().any(|failure| failure.diff_id == item.id) {
            item.status = "failed".to_string();
        }
    }

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );
    orch.emit_review_action_log(
        &app_handle,
        if failed.is_empty() { "success" } else { "warn" },
        "diff_apply",
        &format!(
            "Apply diff {}: {} applied, {} failed",
            diff_id,
            result.applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

/// Apply one hunk from a pending diff to the filesystem.
#[tauri::command]
pub async fn apply_diff_hunk(
    diff_id: String,
    hunk_index: usize,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch.diffs.iter().find(|item| item.id == diff_id).cloned() else {
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

    let single_hunk_diff = FileDiff {
        hunks: vec![hunk],
        ..diff.clone()
    };
    let result = apply_pending_diffs(&[single_hunk_diff]);
    let failed = result.failed.clone();

    if let Some(item) = orch.diffs.iter_mut().find(|item| item.id == diff_id) {
        if result.applied.iter().any(|applied| applied.id == item.id) {
            if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                hunk.status = Some("applied".to_string());
            }
            item.status = status_from_hunks(&item.hunks);
        } else if let Some(failure) = failed.iter().find(|failure| failure.diff_id == item.id) {
            if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                hunk.status = Some("failed".to_string());
            }
            item.status = "failed".to_string();
            let _ = failure;
        }
    }

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );
    orch.emit_review_action_log(
        &app_handle,
        if failed.is_empty() { "success" } else { "warn" },
        "diff_apply",
        &format!(
            "Apply hunk {} in diff {}: {} applied, {} failed",
            hunk_index + 1,
            diff_id,
            result.applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

fn is_cancelled_error(err: &str) -> bool {
    err == "Agent task cancelled"
}

/// Reject all pending diffs.
#[tauri::command]
pub async fn reject_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.reject_diffs();

    orch.state_mgr
        .transition(&crate::agent::state_machine::AgentEvent::UserReject);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );

    let rejected: Vec<FileDiff> = orch
        .diffs
        .iter()
        .filter(|d| d.status == "rejected")
        .cloned()
        .collect();
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!(
            "Rejected {} diff{}",
            rejected.len(),
            if rejected.len() == 1 { "" } else { "s" }
        ),
        &format_diff_list_details(&rejected),
    );

    Ok(rejected)
}

/// Reject one pending diff.
#[tauri::command]
pub async fn reject_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<FileDiff, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch.diffs.iter_mut().find(|item| item.id == diff_id) else {
        return Err(format!("Diff not found: {}", diff_id));
    };

    if diff.status != "pending" {
        return Err(format!(
            "Diff {} is not pending; current status is {}",
            diff_id, diff.status
        ));
    }

    diff.status = "rejected".to_string();
    let rejected = diff.clone();

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!("Rejected diff {}", diff_id),
        &format_diff_list_details(std::slice::from_ref(&rejected)),
    );

    Ok(rejected)
}

/// Reject one hunk from a pending diff.
#[tauri::command]
pub async fn reject_diff_hunk(
    diff_id: String,
    hunk_index: usize,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<FileDiff, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(diff) = orch.diffs.iter_mut().find(|item| item.id == diff_id) else {
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
    hunk.status = Some("rejected".to_string());
    diff.status = status_from_hunks(&diff.hunks);
    let updated = diff.clone();

    set_review_state_after_single_diff(&mut orch);
    let _ = app_handle.emit(
        "agent-state-changed",
        serde_json::json!({ "state": orch.state_mgr.state.to_string() }),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!("Rejected hunk {} in diff {}", hunk_index + 1, diff_id),
        &format_diff_list_details(std::slice::from_ref(&updated)),
    );

    Ok(updated)
}

fn format_apply_result_details(result: &ApplyDiffsResult) -> String {
    let mut lines = Vec::new();
    for diff in &result.applied {
        lines.push(format!("Applied: {} ({})", diff.file, diff.id));
    }
    for failure in &result.failed {
        lines.push(format!(
            "Failed: {} ({}) - {}",
            failure.file, failure.diff_id, failure.message
        ));
    }
    if lines.is_empty() {
        "No matching pending diffs were changed.".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_diff_list_details(diffs: &[FileDiff]) -> String {
    if diffs.is_empty() {
        return "No diffs.".to_string();
    }
    diffs
        .iter()
        .map(|diff| {
            format!(
                "{} ({}) - {} hunk{}",
                diff.file,
                diff.id,
                diff.hunks.len(),
                if diff.hunks.len() == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn status_from_hunks(hunks: &[crate::agent::state_machine::DiffHunk]) -> String {
    if !hunks.is_empty()
        && hunks
            .iter()
            .all(|hunk| matches!(hunk.status.as_deref(), Some("applied")))
    {
        "applied".to_string()
    } else if !hunks.is_empty()
        && hunks
            .iter()
            .all(|hunk| matches!(hunk.status.as_deref(), Some("rejected")))
    {
        "rejected".to_string()
    } else if hunks
        .iter()
        .any(|hunk| matches!(hunk.status.as_deref(), Some("failed")))
    {
        "failed".to_string()
    } else if hunks
        .iter()
        .any(|hunk| matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")))
    {
        "partial".to_string()
    } else {
        "pending".to_string()
    }
}

fn set_review_state_after_single_diff(orch: &mut AgentOrchestrator) {
    if orch
        .diffs
        .iter()
        .any(|diff| diff.status == "pending" || diff.status == "partial" || diff.status == "failed")
    {
        orch.state_mgr.set(AgentState::WaitingUser);
    } else {
        orch.state_mgr.set(AgentState::Done);
    }
}

fn build_agent_context(
    active_file: Option<String>,
    active_file_content: Option<String>,
    selection: Option<String>,
    context_files: Vec<String>,
) -> AgentContext {
    AgentContext {
        active_file,
        active_file_content,
        selection,
        open_files: context_files,
        project_path: workspace::workspace_root_string(),
        git_diff: None,
        project_tree: None,
    }
}

fn default_context_sources() -> ContextSourceOptions {
    ContextSourceOptions {
        include_project_tree: true,
        include_git_diff: true,
    }
}

fn resolve_context_compression(
    agent_state: &State<'_, AgentGlobalState>,
    requested: Option<&str>,
) -> Result<ContextCompressionMode, String> {
    match requested {
        Some(mode) => ContextCompressionMode::from_str(mode),
        None => agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())
            .map(|mode| mode.clone()),
    }
}

fn upsert_step(steps: &mut Vec<TaskStep>, step: TaskStep) {
    if let Some(existing) = steps.iter_mut().find(|item| item.id == step.id) {
        *existing = step;
    } else {
        steps.push(step);
    }
}

fn upsert_step_status(steps: &mut Vec<TaskStep>, step: &TaskStep, status: &str, log: &str) {
    let mut updated = step.clone();
    updated.status = status.to_string();
    updated.logs.push(log.to_string());
    upsert_step(steps, updated);
}

#[cfg(test)]
mod llm_profile_tests {
    use super::*;

    fn test_hunk(status: Option<&str>) -> crate::agent::state_machine::DiffHunk {
        crate::agent::state_machine::DiffHunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            content: "line".to_string(),
            original: "line".to_string(),
            updated: "line".to_string(),
            provenance: None,
            status: status.map(|value| value.to_string()),
        }
    }

    #[test]
    fn hunk_status_rollup_keeps_partial_reviewable() {
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(None)]),
            "partial"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(Some("rejected"))]),
            "partial"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("rejected")), test_hunk(Some("rejected"))]),
            "rejected"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(Some("applied"))]),
            "applied"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("failed")), test_hunk(None)]),
            "failed"
        );
    }

    #[test]
    fn step_prompt_includes_scope_and_mode() {
        let step = TaskStep {
            id: "s1".to_string(),
            title: "Fix parser".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("active_file".to_string()),
            execution_mode: Some("fix".to_string()),
        };

        let prompt = agent_runtime::format_single_step_prompt(&step, Some("Use more context"));

        assert!(prompt.contains("Fix parser"));
        assert!(prompt.contains("Scope: active_file"));
        assert!(prompt.contains("Execution mode: fix"));
        assert!(prompt.contains("Use more context"));
    }

    #[test]
    fn step_provenance_records_regeneration_source() {
        let step = TaskStep {
            id: "s1".to_string(),
            title: "Regenerate stale hunk".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: None,
            execution_mode: None,
        };
        let mut diffs = vec![FileDiff {
            id: "d2".to_string(),
            file: "src/app.ts".to_string(),
            base_hash: None,
            provenance: None,
            hunks: Vec::new(),
            status: "pending".to_string(),
        }];

        agent_runtime::attach_step_provenance(&mut diffs, &step, Some("d1"), Some(2));

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("agent-step"));
        assert_eq!(provenance.regenerated_from_diff_id.as_deref(), Some("d1"));
        assert_eq!(provenance.regenerated_from_hunk_index, Some(2));
    }

    #[test]
    fn plan_mode_uses_dedicated_designer_pipeline() {
        let stages = crate::agent::multi_agent::plan_pipeline();

        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].role, AgentRole::Designer);
        assert_eq!(stages[1].role, AgentRole::Reviewer);
    }
}

/// Get the current steps.
#[tauri::command]
pub async fn get_agent_steps(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.steps.clone())
}

/// Get the current diffs.
#[tauri::command]
pub async fn get_agent_diffs(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.diffs.clone())
}

#[tauri::command]
pub async fn get_agent_sdd_artifacts(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<SddArtifact>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.sdd_artifacts.clone())
}

#[tauri::command]
pub async fn save_sdd_artifact(
    request: SaveSddArtifactRequest,
) -> Result<SavedSddArtifactResponse, String> {
    if !executor::is_safe_slug(&request.artifact.slug) {
        return Err(format!("Invalid SDD slug: {}", request.artifact.slug));
    }
    let relative = format!("docs/design/{}.md", request.artifact.slug);
    let path = workspace::resolve_for_write(&relative)?;
    if path.exists() && !request.overwrite {
        return Err(format!("SDD already exists: {}", relative));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Create SDD directory: {}", e))?;
    }
    std::fs::write(&path, &request.artifact.markdown)
        .map_err(|e| format!("Write SDD artifact: {}", e))?;
    Ok(SavedSddArtifactResponse {
        path: path.to_string_lossy().to_string(),
        artifact: request.artifact,
    })
}

/// Update LLM configuration.
#[tauri::command]
pub async fn update_llm_config(
    endpoint: String,
    api_key: String,
    model: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    {
        let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
        llm_profiles::update_default_profile(&mut config, endpoint, api_key, model, compression)?;
    }

    Ok(())
}

/// Get LLM configuration with the API key masked.
#[derive(Debug, Serialize)]
pub struct LlmConfigResponse {
    pub endpoint: String,
    pub api_key_masked: String,
    pub model: String,
    pub context_compression: String,
    pub profiles: Vec<LlmProfileResponse>,
    pub active_profile_id: String,
}

#[tauri::command]
pub async fn get_llm_config(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmConfigResponse, String> {
    let config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    let active = config
        .profiles
        .iter()
        .find(|profile| profile.id == config.active_profile_id)
        .or_else(|| config.profiles.first())
        .ok_or_else(|| "LLM config not set".to_string())?;
    Ok(LlmConfigResponse {
        endpoint: active.endpoint.clone(),
        api_key_masked: active.masked_api_key(),
        model: active.model.clone(),
        context_compression: agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())?
            .to_string(),
        profiles: config
            .profiles
            .iter()
            .map(|profile| profile.to_response())
            .collect(),
        active_profile_id: config.active_profile_id.clone(),
    })
}

#[tauri::command]
pub async fn save_llm_profile(
    request: SaveLlmProfileRequest,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::save_profile(&mut config, request)
}

#[tauri::command]
pub async fn set_active_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::set_active_profile(&mut config, profile_id)
}

#[tauri::command]
pub async fn delete_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::delete_profile(&mut config, profile_id)
}

#[tauri::command]
pub async fn set_context_compression(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = ContextCompressionMode::from_str(&mode)?;
    {
        let mut current = agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())?;
        *current = parsed.clone();
    }
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::set_context_compression_mode(&mut config, parsed.clone());
    Ok(parsed.to_string())
}

/// Save the workspace path to disk.
#[tauri::command]
pub fn save_workspace_path(path: String) -> Result<(), String> {
    let resolved = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e))?;
    if !resolved.is_dir() {
        return Err(format!("Workspace is not a directory: {}", path));
    }
    workspace::save_workspace_path(&resolved.to_string_lossy())
}

/// Load the last saved workspace path from disk.
#[tauri::command]
pub fn get_workspace_path() -> Result<Option<String>, String> {
    workspace::load_workspace_path()
}

/// Test LLM connectivity with a small request.
#[tauri::command]
pub async fn test_llm_connection(
    agent_state: State<'_, AgentGlobalState>,
    profile_id: Option<String>,
) -> Result<String, String> {
    let llm = agent_state.get_llm_client(profile_id.as_deref())?;

    let messages = vec![crate::services::llm_client::ChatMessage {
        role: "user".to_string(),
        content: "Hi".to_string(),
    }];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
    let handle = tokio::spawn(async move {
        let mut full = String::new();
        while let Some(tok) = rx.recv().await {
            full.push_str(&tok);
        }
        full
    });

    match llm
        .stream_chat(messages, agent_state.cancel_flag.clone(), tx)
        .await
    {
        Ok(response) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let full = handle.await.unwrap_or(response);
            Ok(format!("OK - {}", &full[..full.len().min(120)]))
        }
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

/// Set the current Agent role.
#[tauri::command]
pub async fn set_active_role(
    role: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = match role.as_str() {
        "architect" => AgentRole::Architect,
        "designer" => AgentRole::Designer,
        "coder" => AgentRole::Coder,
        "tester" => AgentRole::Tester,
        "reviewer" => AgentRole::Reviewer,
        _ => return Err(format!("Invalid role: {}", role)),
    };
    let mut active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    *active = parsed;
    Ok(parsed.to_string().to_string())
}

/// Get the current Agent role.
#[tauri::command]
pub async fn get_active_role(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    let active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    Ok(active.to_string().to_string())
}

/// Get the current pipeline.
#[tauri::command]
pub async fn get_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let stages = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(stages.clone())
}

/// Update the pipeline.
#[tauri::command]
pub async fn update_pipeline(
    stages: Vec<PipelineStage>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = stages;
    Ok(())
}

/// Reset the pipeline to defaults.
#[tauri::command]
pub async fn reset_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = default_pipeline();
    Ok(pipe.clone())
}
