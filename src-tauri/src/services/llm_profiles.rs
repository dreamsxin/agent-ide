use crate::services::context::{ContextBudget, ContextCompressionMode};
use crate::services::credentials;
use crate::services::llm_client::LlmConfig;
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
    #[serde(rename = "setActive")]
    pub set_active: Option<bool>,
}

impl LlmProfile {
    pub fn to_config(&self) -> Result<LlmConfig, String> {
        Ok(LlmConfig {
            endpoint: self.endpoint.clone(),
            api_key: self.api_key()?,
            model: self.model.clone(),
            provider: self.provider.clone(),
            max_output_tokens: self.max_output_tokens,
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
            effective_input_tokens: self.effective_input_tokens(),
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
        self.credential_ref
            .as_ref()
            .map(|_| "stored in OS credential store".to_string())
            .unwrap_or_else(|| "not configured".to_string())
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

pub fn context_budget(config: &LlmProfilesConfig, profile_id: Option<&str>) -> Option<ContextBudget> {
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
    if request.name.trim().is_empty()
        || request.endpoint.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Err("Profile name, endpoint, and model are required".to_string());
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
    if let Some(profile) = config.profiles.iter().find(|profile| profile.id == profile_id) {
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
}
