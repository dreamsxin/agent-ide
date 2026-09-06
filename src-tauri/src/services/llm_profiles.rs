use crate::services::context::{ContextBudget, ContextCompressionMode};
use crate::services::credentials;
use crate::services::llm_client::{LlmConfig, LocalModelConfig, ModelType};
use crate::services::workspace;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PROFILE_ID: &str = "default";

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
    /// 单次运行允许消耗的总 token 上限（prompt + completion）。None 表示不限制。
    #[serde(default, rename = "maxRunTokens")]
    pub max_run_tokens: Option<u64>,
    #[serde(default = "default_tool_call_mode", rename = "toolCallMode")]
    pub tool_call_mode: String,
    #[serde(default, rename = "modelType")]
    pub model_type: Option<String>,
    #[serde(default, rename = "modelPath")]
    pub model_path: Option<String>,
    #[serde(default, rename = "modelFile")]
    pub model_file: Option<String>,
    #[serde(default, rename = "nThreads")]
    pub n_threads: Option<i32>,
    #[serde(default, rename = "nCtx")]
    pub n_ctx: Option<u32>,
    #[serde(default, rename = "nGpuLayers")]
    pub n_gpu_layers: Option<i32>,
    #[serde(default, rename = "nBatch")]
    pub n_batch: Option<i32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "topP")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "topK")]
    pub top_k: Option<i32>,
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: Option<u32>,
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
    #[serde(rename = "maxRunTokens")]
    pub max_run_tokens: Option<u64>,
    #[serde(rename = "effectiveInputTokens")]
    pub effective_input_tokens: Option<u32>,
    #[serde(rename = "toolCallMode")]
    pub tool_call_mode: String,
    #[serde(rename = "modelType")]
    pub model_type: Option<String>,
    #[serde(rename = "modelPath")]
    pub model_path: Option<String>,
    #[serde(rename = "modelFile")]
    pub model_file: Option<String>,
    #[serde(rename = "nThreads")]
    pub n_threads: Option<i32>,
    #[serde(rename = "nCtx")]
    pub n_ctx: Option<u32>,
    #[serde(rename = "nGpuLayers")]
    pub n_gpu_layers: Option<i32>,
    #[serde(rename = "nBatch")]
    pub n_batch: Option<i32>,
    pub temperature: Option<f32>,
    #[serde(rename = "topP")]
    pub top_p: Option<f32>,
    #[serde(rename = "topK")]
    pub top_k: Option<i32>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct LlmProfilesResponse {
    pub profiles: Vec<LlmProfileResponse>,
    pub active_profile_id: String,
    pub context_compression: String,
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
    #[serde(rename = "maxRunTokens")]
    pub max_run_tokens: Option<u64>,
    #[serde(rename = "toolCallMode")]
    pub tool_call_mode: Option<String>,
    #[serde(rename = "setActive")]
    pub set_active: Option<bool>,
    #[serde(rename = "modelType")]
    pub model_type: Option<String>,
    #[serde(rename = "modelPath")]
    pub model_path: Option<String>,
    #[serde(rename = "modelFile")]
    pub model_file: Option<String>,
    #[serde(rename = "nThreads")]
    pub n_threads: Option<i32>,
    #[serde(rename = "nCtx")]
    pub n_ctx: Option<u32>,
    #[serde(rename = "nGpuLayers")]
    pub n_gpu_layers: Option<i32>,
    #[serde(rename = "nBatch")]
    pub n_batch: Option<i32>,
    pub temperature: Option<f32>,
    #[serde(rename = "topP")]
    pub top_p: Option<f32>,
    #[serde(rename = "topK")]
    pub top_k: Option<i32>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
}

impl LlmProfile {
    pub fn to_config(&self) -> Result<LlmConfig, String> {
        let model_type = self
            .model_type
            .as_deref()
            .map(ModelType::from_string)
            .unwrap_or_else(|| ModelType::from_string(&self.model));
        let is_local = self.provider.eq_ignore_ascii_case("local") || model_type.is_local();
        let local_model_config = if is_local {
            Some(LocalModelConfig {
                name: self.model.clone(),
                model_type: model_type.clone(),
                model_path: self
                    .model_path
                    .clone()
                    .or_else(|| model_type.default_model_path())
                    .unwrap_or_default(),
                model_file: self.model_file.clone().unwrap_or_default(),
                enabled: true,
                n_threads: self.n_threads.unwrap_or(4),
                n_ctx: self.n_ctx.unwrap_or(4096),
                n_gpu_layers: self.n_gpu_layers.unwrap_or(0),
                n_batch: self.n_batch.unwrap_or(512),
                temperature: self.temperature.unwrap_or(0.2),
                top_p: self.top_p.unwrap_or(0.9),
                top_k: self.top_k.unwrap_or(40),
                max_tokens: self.max_tokens.or(self.max_output_tokens).unwrap_or(512),
            })
        } else {
            None
        };
        Ok(LlmConfig {
            endpoint: self.endpoint.clone(),
            api_key: if is_local {
                String::new()
            } else {
                self.api_key()?
            },
            model: self.model.clone(),
            provider: self.provider.clone(),
            max_output_tokens: self.max_output_tokens,
            tool_call_mode: normalized_tool_call_mode(&self.tool_call_mode),
            model_type,
            local_model_config,
        })
    }

    pub fn to_response(&self) -> LlmProfileResponse {
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
            max_run_tokens: self.max_run_tokens,
            effective_input_tokens: self.effective_input_tokens(),
            tool_call_mode: normalized_tool_call_mode(&self.tool_call_mode),
            model_type: self.model_type.clone(),
            model_path: self.model_path.clone(),
            model_file: self.model_file.clone(),
            n_threads: self.n_threads,
            n_ctx: self.n_ctx,
            n_gpu_layers: self.n_gpu_layers,
            n_batch: self.n_batch,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            max_tokens: self.max_tokens,
        }
    }

    pub fn effective_input_tokens(&self) -> Option<u32> {
        let max_context = self.max_context_tokens?;
        let reserved = self
            .reserved_output_tokens
            .or(self.max_output_tokens)
            .unwrap_or(4096);
        Some(max_context.saturating_sub(reserved).saturating_sub(512))
    }

    pub fn api_key(&self) -> Result<String, String> {
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

    pub fn masked_api_key(&self) -> String {
        if !self.api_key.trim().is_empty() {
            return mask_api_key(&self.api_key);
        }
        match self.credential_ref.as_deref() {
            // 实际探测条目是否可读，而不是"有 credentialRef 就当成已保存"。
            // 后者会在写入失败时谎报密钥已存在，用户看到 "Enter to overwrite"
            // 于是留空保存，陷入永远修不好的循环。
            Some(credential_ref) => match credentials::read_secret(credential_ref) {
                Ok(secret) => mask_api_key(&secret),
                Err(_) => "not configured".to_string(),
            },
            None => "not configured".to_string(),
        }
    }

    /// 该 profile 是否真的有可读的密钥
    pub fn has_readable_api_key(&self) -> bool {
        if !self.api_key.trim().is_empty() {
            return true;
        }
        self.credential_ref
            .as_deref()
            .is_some_and(credentials::has_secret)
    }
}

pub fn save_llm_config_to_disk(config: &LlmProfilesConfig) {
    let dir = workspace::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.json");
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, json);
    }
}

pub fn load_llm_config_from_disk() -> Option<LlmProfilesConfig> {
    let path = workspace::config_dir().join("config.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let (config, credentials_migrated) = parse_llm_profiles_config_with_migration(parsed)?;
    if credentials_migrated {
        save_llm_config_to_disk(&config);
    }
    Some(config)
}

pub fn load_or_default_config() -> LlmProfilesConfig {
    load_llm_config_from_disk().unwrap_or_else(default_config_from_env)
}

fn default_config_from_env() -> LlmProfilesConfig {
    let endpoint =
        std::env::var("LLM_ENDPOINT").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());
    let mode = std::env::var("AGENT_CONTEXT_COMPRESSION")
        .ok()
        .and_then(|value| ContextCompressionMode::from_str(&value).ok())
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
            max_run_tokens: None,
            tool_call_mode: default_tool_call_mode(),
            model_type: None,
            model_path: None,
            model_file: None,
            n_threads: None,
            n_ctx: None,
            n_gpu_layers: None,
            n_batch: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
        }],
        active_profile_id: DEFAULT_PROFILE_ID.to_string(),
        context_compression: mode,
    }
}

#[cfg(test)]
pub fn parse_llm_profiles_config(parsed: serde_json::Value) -> Option<LlmProfilesConfig> {
    parse_llm_profiles_config_with_migration(parsed).map(|(config, _)| config)
}

fn parse_llm_profiles_config_with_migration(
    parsed: serde_json::Value,
) -> Option<(LlmProfilesConfig, bool)> {
    let context_compression = parsed
        .get("context_compression")
        .and_then(|value| value.as_str())
        .and_then(|value| ContextCompressionMode::from_str(value).ok())
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
            .and_then(|value| value.as_str())
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
        max_run_tokens: None,
        tool_call_mode: default_tool_call_mode(),
        model_type: None,
        model_path: None,
        model_file: None,
        n_threads: None,
        n_ctx: None,
        n_gpu_layers: None,
        n_batch: None,
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: None,
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

pub fn resolve_llm_config(
    config: &LlmProfilesConfig,
    profile_id: Option<&str>,
) -> Result<LlmConfig, String> {
    let selected_id = profile_id.unwrap_or(&config.active_profile_id);
    let profile = config
        .profiles
        .iter()
        .find(|profile| profile.id == selected_id)
        .or_else(|| config.profiles.first())
        .ok_or_else(|| "LLM profile not configured".to_string())?;
    profile.to_config()
}

/// 读出某个 profile 的明文密钥，供设置面板的"显示"按钮使用。
///
/// 刻意做成独立入口而不是塞进 `to_response()`：例行的 profile 列表响应
/// 不应该携带明文密钥，只有用户显式点击时才取一次。
pub fn reveal_api_key(
    config: &LlmProfilesConfig,
    profile_id: Option<&str>,
) -> Result<String, String> {
    let selected_id = profile_id.unwrap_or(&config.active_profile_id);
    let profile = config
        .profiles
        .iter()
        .find(|profile| profile.id == selected_id)
        .or_else(|| config.profiles.first())
        .ok_or_else(|| "LLM profile not configured".to_string())?;
    profile.api_key()
}

pub fn context_budget(
    config: &LlmProfilesConfig,
    profile_id: Option<&str>,
) -> Option<ContextBudget> {
    let selected_id = profile_id.unwrap_or(&config.active_profile_id);
    let profile = config
        .profiles
        .iter()
        .find(|profile| profile.id == selected_id)
        .or_else(|| config.profiles.first())?;
    if profile.max_context_tokens.is_none() && profile.reserved_output_tokens.is_none() {
        return None;
    }
    Some(ContextBudget {
        max_context_tokens: profile.max_context_tokens.map(|value| value as usize),
        reserved_output_tokens: profile.reserved_output_tokens.map(|value| value as usize),
    })
}

/// 单次运行的 token 上限。未配置时返回 None（不限流）。
pub fn run_token_cap(config: &LlmProfilesConfig, profile_id: Option<&str>) -> Option<u64> {
    let selected_id = profile_id.unwrap_or(&config.active_profile_id);
    config
        .profiles
        .iter()
        .find(|profile| profile.id == selected_id)
        .or_else(|| config.profiles.first())
        .and_then(|profile| profile.max_run_tokens)
        // 0 当作"没设置"，避免手写配置时一个 0 把所有运行直接锁死
        .filter(|cap| *cap > 0)
}

pub fn update_default_profile(
    config: &mut LlmProfilesConfig,
    endpoint: String,
    api_key: String,
    model: String,
    compression: ContextCompressionMode,
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
        max_run_tokens: None,
        tool_call_mode: default_tool_call_mode(),
        model_type: None,
        model_path: None,
        model_file: None,
        n_threads: None,
        n_ctx: None,
        n_gpu_layers: None,
        n_batch: None,
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: None,
    };
    credentials::store_secret(
        &credentials::llm_credential_ref(DEFAULT_PROFILE_ID),
        &api_key,
    )?;
    upsert_profile(&mut config.profiles, profile);
    config.active_profile_id = DEFAULT_PROFILE_ID.to_string();
    config.context_compression = compression;
    save_llm_config_to_disk(config);
    Ok(())
}

pub fn save_profile(
    config: &mut LlmProfilesConfig,
    request: SaveLlmProfileRequest,
) -> Result<LlmProfilesResponse, String> {
    if request.name.trim().is_empty() || request.model.trim().is_empty() {
        return Err("Profile name and model are required".to_string());
    }
    let is_local = request.provider.eq_ignore_ascii_case("local")
        || request
            .model_type
            .as_deref()
            .map(ModelType::from_string)
            .map(|model_type| model_type.is_local())
            .unwrap_or(false);
    if !is_local && request.endpoint.trim().is_empty() {
        return Err("Endpoint is required for cloud profiles".to_string());
    }
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
    // 校验依据是"现有密钥是否真的读得出来"，而不是"credentialRef 是否存在"。
    // 引用存在但条目不可读时必须要求重新输入，否则保存会成功但运行时永远失败。
    if !is_local
        && api_key.trim().is_empty()
        && !existing_profile
            .as_ref()
            .is_some_and(|profile| profile.has_readable_api_key())
    {
        return Err(
            "Secret key is required: no readable key is stored for this profile".to_string(),
        );
    }
    if !api_key.trim().is_empty() {
        credentials::store_secret(&credential_ref, &api_key)?;
    }
    let profile = LlmProfile {
        id: id.clone(),
        name: request.name.trim().to_string(),
        provider: request.provider.trim().to_string(),
        endpoint: request.endpoint.trim().to_string(),
        credential_ref: if is_local { None } else { Some(credential_ref) },
        api_key: String::new(),
        model: request.model.trim().to_string(),
        max_context_tokens: request.max_context_tokens,
        reserved_output_tokens: request.reserved_output_tokens,
        max_output_tokens: request.max_output_tokens,
        max_run_tokens: request.max_run_tokens,
        tool_call_mode: request
            .tool_call_mode
            .as_deref()
            .map(normalized_tool_call_mode)
            .unwrap_or_else(|| {
                if is_local {
                    "text_protocol".to_string()
                } else {
                    default_tool_call_mode()
                }
            }),
        model_type: if is_local {
            Some(
                request
                    .model_type
                    .unwrap_or_else(|| ModelType::from_string(&request.model).to_string()),
            )
        } else {
            None
        },
        model_path: request.model_path,
        model_file: request.model_file,
        n_threads: request.n_threads,
        n_ctx: request.n_ctx,
        n_gpu_layers: request.n_gpu_layers,
        n_batch: request.n_batch,
        temperature: request.temperature,
        top_p: request.top_p,
        top_k: request.top_k,
        max_tokens: request.max_tokens,
    };
    upsert_profile(&mut config.profiles, profile);
    if request.set_active.unwrap_or(true) {
        config.active_profile_id = id;
    }
    save_llm_config_to_disk(config);
    Ok(profiles_response(config))
}

pub fn set_active_profile(
    config: &mut LlmProfilesConfig,
    profile_id: String,
) -> Result<LlmProfilesResponse, String> {
    if !config
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id)
    {
        return Err(format!("LLM profile not found: {}", profile_id));
    }
    config.active_profile_id = profile_id;
    save_llm_config_to_disk(config);
    Ok(profiles_response(config))
}

pub fn delete_profile(
    config: &mut LlmProfilesConfig,
    profile_id: String,
) -> Result<LlmProfilesResponse, String> {
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
    save_llm_config_to_disk(config);
    Ok(profiles_response(config))
}

pub fn set_context_compression_mode(
    config: &mut LlmProfilesConfig,
    parsed: ContextCompressionMode,
) {
    config.context_compression = parsed;
    save_llm_config_to_disk(config);
}

pub fn profiles_response(config: &LlmProfilesConfig) -> LlmProfilesResponse {
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

pub fn infer_provider(endpoint: &str) -> &'static str {
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

fn mask_api_key(api_key: &str) -> String {
    if api_key.len() > 8 {
        format!("{}****{}", &api_key[..4], &api_key[api_key.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn chrono_like_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn default_tool_call_mode() -> String {
    "text_protocol".to_string()
}

fn normalized_tool_call_mode(value: &str) -> String {
    match value.trim() {
        "native_tools" => "native_tools".to_string(),
        _ => "text_protocol".to_string(),
    }
}

#[cfg(test)]
mod tests {
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
            max_run_tokens: Some(250_000),
            tool_call_mode: "native_tools".to_string(),
            model_type: None,
            model_path: None,
            model_file: None,
            n_threads: None,
            n_ctx: None,
            n_gpu_layers: None,
            n_batch: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
        };

        assert_eq!(profile.to_response().api_key_masked, "sk-1****7890");
        assert_eq!(profile.to_response().effective_input_tokens, Some(123392));
        assert_eq!(profile.to_response().tool_call_mode, "native_tools");
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
            max_run_tokens: None,
            tool_call_mode: default_tool_call_mode(),
            model_type: None,
            model_path: None,
            model_file: None,
            n_threads: None,
            n_ctx: None,
            n_gpu_layers: None,
            n_batch: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
        };

        let serialized = serde_json::to_value(&profile).expect("serialize profile");

        assert_eq!(serialized["credentialRef"], "llm-profile:p1");
        assert_eq!(serialized["toolCallMode"], "text_protocol");
        assert!(serialized.get("api_key").is_none());
    }

    #[test]
    fn run_token_cap_reads_the_selected_profile_and_ignores_zero() {
        let profile: LlmProfile = serde_json::from_value(serde_json::json!({
            "id": "capped",
            "name": "Capped",
            "provider": "openai",
            "endpoint": "https://api.openai.com/v1",
            "model": "gpt-4o",
            "maxRunTokens": 120000
        }))
        .expect("profile");
        let mut zeroed = profile.clone();
        zeroed.id = "zeroed".to_string();
        // 手写配置里一个 0 不该把所有运行直接锁死
        zeroed.max_run_tokens = Some(0);
        let mut unset = profile.clone();
        unset.id = "unset".to_string();
        unset.max_run_tokens = None;

        let config = LlmProfilesConfig {
            profiles: vec![profile, zeroed, unset],
            active_profile_id: "capped".to_string(),
            context_compression: ContextCompressionMode::default(),
        };

        assert_eq!(run_token_cap(&config, None), Some(120_000));
        assert_eq!(run_token_cap(&config, Some("capped")), Some(120_000));
        assert_eq!(run_token_cap(&config, Some("zeroed")), None);
        assert_eq!(run_token_cap(&config, Some("unset")), None);
    }

    #[test]
    fn profile_deserialization_defaults_to_text_protocol_tools() {
        let profile: LlmProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "Work",
            "provider": "openai",
            "endpoint": "https://api.openai.com/v1",
            "model": "gpt-4o"
        }))
        .expect("profile");

        assert_eq!(profile.tool_call_mode, "text_protocol");
    }
}
