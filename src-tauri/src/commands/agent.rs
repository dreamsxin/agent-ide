use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::multi_agent::{default_pipeline, AgentRole, PipelineStage};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::agent::state_machine::{
    AgentMode, AgentState, ApplyDiffsResult, DiffProvenance, FileDiff, TaskStep,
};
use crate::services::context::{
    AgentContext, ContextBudget, ContextBuildOptions, ContextCompressionMode,
    ContextEstimateResponse, ContextSourceOptions,
};
use crate::services::credentials;
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Emitter;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

const DEFAULT_PROFILE_ID: &str = "default";

/// Global Agent state. Uses tokio::sync::Mutex for async orchestration.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    #[serde(default, rename = "credentialRef")]
    pub credential_ref: Option<String>,
    #[serde(default, skip_serializing)]
    pub api_key: String,
    pub model: String,
    #[serde(default, rename = "maxContextTokens")]
    pub max_context_tokens: Option<u32>,
    #[serde(default, rename = "reservedOutputTokens")]
    pub reserved_output_tokens: Option<u32>,
    #[serde(default, rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProfilesConfig {
    pub profiles: Vec<LlmProfile>,
    pub active_profile_id: String,
    pub context_compression: ContextCompressionMode,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmProfileResponse {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    pub api_key_masked: String,
    pub model: String,
    #[serde(rename = "maxContextTokens")]
    pub max_context_tokens: Option<u32>,
    #[serde(rename = "reservedOutputTokens")]
    pub reserved_output_tokens: Option<u32>,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "effectiveInputTokens")]
    pub effective_input_tokens: Option<u32>,
}

impl LlmProfile {
    fn to_config(&self) -> Result<LlmConfig, String> {
        Ok(LlmConfig {
            endpoint: self.endpoint.clone(),
            api_key: self.api_key()?,
            model: self.model.clone(),
            provider: self.provider.clone(),
            max_output_tokens: self.max_output_tokens,
        })
    }

    fn to_response(&self) -> LlmProfileResponse {
        LlmProfileResponse {
            id: self.id.clone(),
            name: self.name.clone(),
            provider: self.provider.clone(),
            endpoint: self.endpoint.clone(),
            api_key_masked: self.masked_api_key(),
            model: self.model.clone(),
            max_context_tokens: self.max_context_tokens,
            reserved_output_tokens: self.reserved_output_tokens,
            max_output_tokens: self.max_output_tokens,
            effective_input_tokens: self.effective_input_tokens(),
        }
    }

    fn effective_input_tokens(&self) -> Option<u32> {
        let max_context = self.max_context_tokens?;
        let reserved = self
            .reserved_output_tokens
            .or(self.max_output_tokens)
            .unwrap_or(4096);
        Some(max_context.saturating_sub(reserved).saturating_sub(512))
    }

    fn api_key(&self) -> Result<String, String> {
        if !self.api_key.trim().is_empty() {
            return Ok(self.api_key.clone());
        }
        let credential_ref = self.credential_ref.as_ref().ok_or_else(|| {
            format!(
                "LLM credential is not configured for profile '{}'",
                self.name
            )
        })?;
        credentials::read_secret(credential_ref)
    }

    fn masked_api_key(&self) -> String {
        if !self.api_key.trim().is_empty() {
            return mask_api_key(&self.api_key);
        }
        self.credential_ref
            .as_ref()
            .map(|_| "stored in OS credential store".to_string())
            .unwrap_or_else(|| "not configured".to_string())
    }
}

/// Save LLM configuration to disk.
fn save_llm_config_to_disk(config: &LlmProfilesConfig) {
    let dir = workspace::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.json");
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, json);
    }
}

/// Load LLM configuration from disk.
fn load_llm_config_from_disk() -> Option<LlmProfilesConfig> {
    let path = workspace::config_dir().join("config.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let (config, credentials_migrated) = parse_llm_profiles_config_with_migration(parsed)?;
    if credentials_migrated {
        save_llm_config_to_disk(&config);
    }
    Some(config)
}

#[cfg(test)]
fn parse_llm_profiles_config(parsed: serde_json::Value) -> Option<LlmProfilesConfig> {
    parse_llm_profiles_config_with_migration(parsed).map(|(config, _)| config)
}

fn parse_llm_profiles_config_with_migration(
    parsed: serde_json::Value,
) -> Option<(LlmProfilesConfig, bool)> {
    let context_compression = parsed
        .get("context_compression")
        .and_then(|v| v.as_str())
        .and_then(|v| ContextCompressionMode::from_str(v).ok())
        .unwrap_or_default();

    if let Some(profiles) = parsed.get("profiles").and_then(|value| value.as_array()) {
        let profiles: Vec<LlmProfile> = profiles
            .iter()
            .filter_map(|profile| serde_json::from_value(profile.clone()).ok())
            .collect();
        if profiles.is_empty() {
            return None;
        }
        let active_profile_id = parsed
            .get("active_profile_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&profiles[0].id)
            .to_string();
        let (profiles, credentials_migrated) = migrate_profile_credentials(profiles);
        return Some((
            LlmProfilesConfig {
                profiles,
                active_profile_id,
                context_compression,
            },
            credentials_migrated,
        ));
    }

    let api_key = parsed.get("api_key")?.as_str()?.to_string();
    let (profiles, credentials_migrated) = migrate_profile_credentials(vec![LlmProfile {
        id: DEFAULT_PROFILE_ID.to_string(),
        name: "Default".to_string(),
        provider: "custom".to_string(),
        endpoint: parsed.get("endpoint")?.as_str()?.to_string(),
        credential_ref: None,
        api_key,
        model: parsed.get("model")?.as_str()?.to_string(),
        max_context_tokens: None,
        reserved_output_tokens: None,
        max_output_tokens: None,
    }]);
    Some((
        LlmProfilesConfig {
            profiles,
            active_profile_id: DEFAULT_PROFILE_ID.to_string(),
            context_compression,
        },
        credentials_migrated,
    ))
}

fn migrate_profile_credentials(mut profiles: Vec<LlmProfile>) -> (Vec<LlmProfile>, bool) {
    let mut credentials_migrated = true;
    for profile in &mut profiles {
        let credential_ref = profile
            .credential_ref
            .clone()
            .unwrap_or_else(|| credentials::llm_credential_ref(&profile.id));
        if !profile.api_key.trim().is_empty() {
            match credentials::store_secret(&credential_ref, &profile.api_key) {
                Ok(()) => {
                    profile.credential_ref = Some(credential_ref);
                    profile.api_key.clear();
                }
                Err(_) => {
                    credentials_migrated = false;
                }
            }
        } else if profile.credential_ref.is_none() {
            profile.credential_ref = Some(credential_ref);
        }
    }
    (profiles, credentials_migrated)
}

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
        let profiles_config = load_llm_config_from_disk().unwrap_or_else(|| {
            let endpoint = std::env::var("LLM_ENDPOINT")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());
            let mode = std::env::var("AGENT_CONTEXT_COMPRESSION")
                .ok()
                .and_then(|v| ContextCompressionMode::from_str(&v).ok())
                .unwrap_or_default();
            let credential_ref = credentials::llm_credential_ref(DEFAULT_PROFILE_ID);
            if !api_key.trim().is_empty() {
                let _ = credentials::store_secret(&credential_ref, &api_key);
            }
            LlmProfilesConfig {
                profiles: vec![LlmProfile {
                    id: DEFAULT_PROFILE_ID.to_string(),
                    name: "Default".to_string(),
                    provider: "openai".to_string(),
                    endpoint,
                    credential_ref: Some(credential_ref),
                    api_key: String::new(),
                    model,
                    max_context_tokens: None,
                    reserved_output_tokens: None,
                    max_output_tokens: None,
                }],
                active_profile_id: DEFAULT_PROFILE_ID.to_string(),
                context_compression: mode,
            }
        });
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
        let selected_id = profile_id.unwrap_or(&profiles.active_profile_id);
        let profile = profiles
            .profiles
            .iter()
            .find(|profile| profile.id == selected_id)
            .or_else(|| profiles.profiles.first())
            .ok_or_else(|| "LLM profile not configured".to_string())?;
        profile.to_config()
    }

    pub fn get_context_budget(&self, profile_id: Option<&str>) -> Option<ContextBudget> {
        let profiles = self.llm_profiles.lock().ok()?;
        let selected_id = profile_id.unwrap_or(&profiles.active_profile_id);
        let profile = profiles
            .profiles
            .iter()
            .find(|profile| profile.id == selected_id)
            .or_else(|| profiles.profiles.first())?;
        if profile.max_context_tokens.is_none() && profile.reserved_output_tokens.is_none() {
            return None;
        }
        Some(ContextBudget {
            max_context_tokens: profile.max_context_tokens.map(|value| value as usize),
            reserved_output_tokens: profile.reserved_output_tokens.map(|value| value as usize),
        })
    }
}

/// Agent status response DTO.
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
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
    orch.steps.clear();
    orch.diffs.clear();
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
    let step_prompt = format_step_prompt(&step, request.extra_prompt.as_deref());

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

            let mut diffs = crate::agent::executor::parse_diffs(&response);
            attach_step_provenance(
                &mut diffs,
                &step,
                request.regenerated_from_diff_id.as_deref(),
                request.regenerated_from_hunk_index,
            );
            orch.diffs.extend(diffs);
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
                        .filter(|diff| diff.status == "pending")
                        .count(),
                    if orch
                        .diffs
                        .iter()
                        .filter(|diff| diff.status == "pending")
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

    if diff.status != "pending" && diff.status != "failed" {
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

    if diff.status != "pending" && diff.status != "failed" {
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
    if hunks
        .iter()
        .all(|hunk| matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")))
    {
        "applied".to_string()
    } else if hunks
        .iter()
        .any(|hunk| matches!(hunk.status.as_deref(), Some("failed")))
    {
        "failed".to_string()
    } else {
        "pending".to_string()
    }
}

fn set_review_state_after_single_diff(orch: &mut AgentOrchestrator) {
    if orch
        .diffs
        .iter()
        .any(|diff| diff.status == "pending" || diff.status == "failed")
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

fn format_step_prompt(step: &TaskStep, extra_prompt: Option<&str>) -> String {
    let mut lines = vec![
        format!("Run this Agent plan step only: {}", step.title),
        format!("Step type: {}", step.step_type),
        format!("Scope: {}", step.scope.as_deref().unwrap_or("workspace")),
        format!(
            "Execution mode: {}",
            step.execution_mode.as_deref().unwrap_or("diff")
        ),
    ];
    if let Some(extra_prompt) = extra_prompt.filter(|value| !value.trim().is_empty()) {
        lines.push("Additional instruction:".to_string());
        lines.push(extra_prompt.to_string());
    }
    lines.push(
        "Return reviewable Agent IDE diffs when code changes are needed. Do not run unrelated plan steps."
            .to_string(),
    );
    lines.join("\n")
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

fn attach_step_provenance(
    diffs: &mut [FileDiff],
    step: &TaskStep,
    regenerated_from_diff_id: Option<&str>,
    regenerated_from_hunk_index: Option<usize>,
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
        provenance.source_role = Some("agent-step".to_string());
        provenance.source_stage = Some(step.title.clone());
        provenance.regenerated_from_diff_id = regenerated_from_diff_id.map(str::to_string);
        provenance.regenerated_from_hunk_index = regenerated_from_hunk_index;
    }
}

fn mask_api_key(api_key: &str) -> String {
    if api_key.len() > 8 {
        format!("{}****{}", &api_key[..4], &api_key[api_key.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn infer_provider(endpoint: &str) -> &'static str {
    if endpoint.contains("openai.azure.com") {
        "azure"
    } else if endpoint.contains("api.openai.com") {
        "openai"
    } else if endpoint.contains("anthropic.com") {
        "anthropic"
    } else if endpoint.contains("deepseek.com") {
        "deepseek"
    } else {
        "custom"
    }
}

fn upsert_profile(profiles: &mut Vec<LlmProfile>, profile: LlmProfile) {
    if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
}

fn profiles_response(config: &LlmProfilesConfig) -> LlmProfilesResponse {
    LlmProfilesResponse {
        profiles: config
            .profiles
            .iter()
            .map(LlmProfile::to_response)
            .collect(),
        active_profile_id: config.active_profile_id.clone(),
        context_compression: config.context_compression.to_string(),
    }
}

fn chrono_like_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod llm_profile_tests {
    use super::*;

    #[test]
    fn legacy_llm_config_migrates_to_default_profile_shape() {
        let parsed: serde_json::Value = serde_json::json!({
            "endpoint": "https://api.deepseek.com",
            "api_key": "sk-test",
            "model": "deepseek-chat",
            "context_compression": "compact"
        });
        let config = parse_llm_profiles_config(parsed).expect("config");

        assert_eq!(config.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(config.context_compression.to_string(), "compact");
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.profiles[0].provider, "custom");
        assert_eq!(config.profiles[0].endpoint, "https://api.deepseek.com");
    }

    #[test]
    fn profile_response_masks_api_key() {
        let profile = LlmProfile {
            id: "p1".to_string(),
            name: "Work".to_string(),
            provider: "openai".to_string(),
            endpoint: "https://api.openai.com/v1".to_string(),
            credential_ref: None,
            api_key: "sk-1234567890".to_string(),
            model: "gpt-4o".to_string(),
            max_context_tokens: Some(128000),
            reserved_output_tokens: Some(4096),
            max_output_tokens: Some(4096),
        };

        assert_eq!(profile.to_response().api_key_masked, "sk-1****7890");
        assert_eq!(profile.to_response().effective_input_tokens, Some(123392));
    }

    #[test]
    fn profile_serialization_omits_plain_api_key() {
        let profile = LlmProfile {
            id: "p1".to_string(),
            name: "Work".to_string(),
            provider: "openai".to_string(),
            endpoint: "https://api.openai.com/v1".to_string(),
            credential_ref: Some("llm-profile:p1".to_string()),
            api_key: "sk-secret".to_string(),
            model: "gpt-4o".to_string(),
            max_context_tokens: None,
            reserved_output_tokens: None,
            max_output_tokens: None,
        };

        let serialized = serde_json::to_value(&profile).expect("serialize profile");

        assert_eq!(serialized["credentialRef"], "llm-profile:p1");
        assert!(serialized.get("api_key").is_none());
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

        let prompt = format_step_prompt(&step, Some("Use more context"));

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

        attach_step_provenance(&mut diffs, &step, Some("d1"), Some(2));

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("agent-step"));
        assert_eq!(provenance.regenerated_from_diff_id.as_deref(), Some("d1"));
        assert_eq!(provenance.regenerated_from_hunk_index, Some(2));
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

/// Update LLM configuration.
#[tauri::command]
pub async fn update_llm_config(
    endpoint: String,
    api_key: String,
    model: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let profile = LlmProfile {
        id: DEFAULT_PROFILE_ID.to_string(),
        name: "Default".to_string(),
        provider: infer_provider(&endpoint).to_string(),
        endpoint,
        credential_ref: Some(credentials::llm_credential_ref(DEFAULT_PROFILE_ID)),
        api_key: String::new(),
        model,
        max_context_tokens: None,
        reserved_output_tokens: None,
        max_output_tokens: None,
    };
    credentials::store_secret(
        &credentials::llm_credential_ref(DEFAULT_PROFILE_ID),
        &api_key,
    )?;
    let compression = agent_state
        .context_compression
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    {
        let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
        upsert_profile(&mut config.profiles, profile);
        config.active_profile_id = DEFAULT_PROFILE_ID.to_string();
        config.context_compression = compression;
        save_llm_config_to_disk(&config);
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
            .map(LlmProfile::to_response)
            .collect(),
        active_profile_id: config.active_profile_id.clone(),
    })
}

#[derive(Debug, Deserialize)]
pub struct SaveLlmProfileRequest {
    pub id: Option<String>,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    pub model: String,
    #[serde(rename = "maxContextTokens")]
    pub max_context_tokens: Option<u32>,
    #[serde(rename = "reservedOutputTokens")]
    pub reserved_output_tokens: Option<u32>,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "setActive")]
    pub set_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LlmProfilesResponse {
    pub profiles: Vec<LlmProfileResponse>,
    pub active_profile_id: String,
    pub context_compression: String,
}

#[tauri::command]
pub async fn save_llm_profile(
    request: SaveLlmProfileRequest,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    if request.name.trim().is_empty()
        || request.endpoint.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Err("Profile name, endpoint, and model are required".to_string());
    }
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    let id = request
        .id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("profile-{}", chrono_like_timestamp()));
    let existing_profile = config
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .cloned();
    let credential_ref = existing_profile
        .as_ref()
        .and_then(|profile| profile.credential_ref.clone())
        .unwrap_or_else(|| credentials::llm_credential_ref(&id));
    let api_key = request
        .api_key
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if api_key.trim().is_empty()
        && existing_profile
            .as_ref()
            .and_then(|profile| profile.credential_ref.as_ref())
            .is_none()
    {
        return Err("API key is required for a new profile".to_string());
    }
    if !api_key.trim().is_empty() {
        credentials::store_secret(&credential_ref, &api_key)?;
    }
    let profile = LlmProfile {
        id: id.clone(),
        name: request.name.trim().to_string(),
        provider: request.provider.trim().to_string(),
        endpoint: request.endpoint.trim().to_string(),
        credential_ref: Some(credential_ref),
        api_key: String::new(),
        model: request.model.trim().to_string(),
        max_context_tokens: request.max_context_tokens,
        reserved_output_tokens: request.reserved_output_tokens,
        max_output_tokens: request.max_output_tokens,
    };
    upsert_profile(&mut config.profiles, profile);
    if request.set_active.unwrap_or(true) {
        config.active_profile_id = id;
    }
    save_llm_config_to_disk(&config);
    Ok(profiles_response(&config))
}

#[tauri::command]
pub async fn set_active_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    if !config
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id)
    {
        return Err(format!("LLM profile not found: {}", profile_id));
    }
    config.active_profile_id = profile_id;
    save_llm_config_to_disk(&config);
    Ok(profiles_response(&config))
}

#[tauri::command]
pub async fn delete_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    if config.profiles.len() <= 1 {
        return Err("At least one LLM profile is required".to_string());
    }
    if let Some(profile) = config
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
    {
        if let Some(credential_ref) = profile.credential_ref.as_ref() {
            let _ = credentials::delete_secret(credential_ref);
        }
    }
    config.profiles.retain(|profile| profile.id != profile_id);
    if config.active_profile_id == profile_id {
        config.active_profile_id = config
            .profiles
            .first()
            .map(|profile| profile.id.clone())
            .unwrap_or_else(|| DEFAULT_PROFILE_ID.to_string());
    }
    save_llm_config_to_disk(&config);
    Ok(profiles_response(&config))
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
    config.context_compression = parsed.clone();
    save_llm_config_to_disk(&config);
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
