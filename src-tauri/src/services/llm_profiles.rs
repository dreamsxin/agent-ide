use crate::services::context::{ContextBudget, ContextCompressionMode};
use crate::services::credentials;
use crate::services::llm_client::{LlmConfig, LocalModelConfig, ModelType, TokenPricing};
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
    /// 每百万 prompt token 的价格，单位微美元（$0.28/M 就写 280000）。
    /// 用整数是为了让钱的累加不带浮点误差。
    #[serde(default, rename = "promptMicrosPerMillion")]
    pub prompt_micros_per_million: Option<u64>,
    #[serde(default, rename = "completionMicrosPerMillion")]
    pub completion_micros_per_million: Option<u64>,
    /// 单次运行的金额上限（微美元）。没配价格时这个上限无法执行 ——
    /// 那种情况会在运行记录里写明"not computable"，而不是当成没有上限。
    #[serde(default, rename = "maxRunSpendMicros")]
    pub max_run_spend_micros: Option<u64>,
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
    /// 这个 profile 现在**能不能真的跑起来**。
    ///
    /// 界面不能靠 `api_key_masked != "not configured"` 这种字符串比较来判断"已配置"：
    /// 一份没被开启的明文密钥会显示成 `sk-1****7890 (plaintext in config.json)`，字符串
    /// 比较于是判成已配置，而每一次运行都会失败 —— 正是那个"显示已保存、留空保存、运行全挂"
    /// 的老陷阱。所以把结论做成一个字段，而不是让前端去猜。
    pub api_key_usable: bool,
    pub model: String,
    #[serde(rename = "maxContextTokens")]
    pub max_context_tokens: Option<u32>,
    #[serde(rename = "reservedOutputTokens")]
    pub reserved_output_tokens: Option<u32>,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "maxRunTokens")]
    pub max_run_tokens: Option<u64>,
    #[serde(rename = "promptMicrosPerMillion")]
    pub prompt_micros_per_million: Option<u64>,
    #[serde(rename = "completionMicrosPerMillion")]
    pub completion_micros_per_million: Option<u64>,
    #[serde(rename = "maxRunSpendMicros")]
    pub max_run_spend_micros: Option<u64>,
    #[serde(rename = "effectiveInputTokens")]
    /// 界面上那行估算。窗口没填就是 `None` —— 见 `LlmProfile::effective_input_tokens`，
    /// 那里解释了为什么不猜一个窗口。
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
    #[serde(rename = "promptMicrosPerMillion")]
    pub prompt_micros_per_million: Option<u64>,
    #[serde(rename = "completionMicrosPerMillion")]
    pub completion_micros_per_million: Option<u64>,
    #[serde(rename = "maxRunSpendMicros")]
    pub max_run_spend_micros: Option<u64>,
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
            // 窗口跟着 profile 走：客户端用它按"这一次还剩多少"夹输出上限，
            // 见 `window_limited_output_tokens`。它仍然不会被发给供应商。
            max_context_tokens: self.max_context_tokens,
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
            api_key_usable: self.has_readable_api_key(),
            model: self.model.clone(),
            max_context_tokens: self.max_context_tokens,
            reserved_output_tokens: self.reserved_output_tokens,
            max_output_tokens: self.max_output_tokens,
            max_run_tokens: self.max_run_tokens,
            prompt_micros_per_million: self.prompt_micros_per_million,
            completion_micros_per_million: self.completion_micros_per_million,
            max_run_spend_micros: self.max_run_spend_micros,
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

    /// 界面上那行"有效输入预算"。
    ///
    /// 窗口没填就是 `None` —— **不猜**。曾经这里套一个"假定 128k"，而现在各家主力模型是
    /// 200k 到 1M，一个全局常量因此在多数情况下都是错的：预算看着只有十几万，实际有一百万，
    /// 于是这一行开始制造假警报，和它本来要防的假信心一样坏。窗口未知时正确的显示是"未知"，
    /// 界面会退回到按模型查表（`utils/modelLimits.ts`），查不到就显示 unknown。
    ///
    /// 预留输出仍然有默认（`DEFAULT_RESERVED_OUTPUT_TOKENS`），它和模型无关，只是"给回答
    /// 留多少"的一个策略值。
    pub fn effective_input_tokens(&self) -> Option<u32> {
        let max_context = self.max_context_tokens?;
        let reserved = self
            .reserved_output_tokens
            .or(self.max_output_tokens)
            .unwrap_or(crate::services::context::DEFAULT_RESERVED_OUTPUT_TOKENS);
        Some(
            max_context
                .saturating_sub(reserved)
                .saturating_sub(crate::services::context::CONTEXT_ASSEMBLY_HEADROOM_TOKENS),
        )
    }

    /// 取出这个 profile 的密钥。**keyring 优先，明文字段不再是兜底。**
    ///
    /// 以前的顺序是"明文字段有值就用它"，那让 keyring 的保证形同虚设：只要有一次迁移失败，
    /// `config.json` 里就永久留着一份明文，而且从此优先于 keyring 里那份 —— 两种存储模型
    /// 的坏处一起吃。ROADMAP 凭据那一节（"Remove the silent plaintext fallback"）定的是
    /// 宁可响亮地失败、让用户重新输一次。
    ///
    /// 明文仍然可用，但必须由用户显式开启（`AGENT_IDE_ALLOW_PLAINTEXT_KEY`）。差别在于
    /// 用户**知不知道**：同一台机器上明文可能完全可以接受，不能接受的是它悄悄发生。
    pub fn api_key(&self) -> Result<String, String> {
        let plaintext = self.api_key.trim();
        if let Some(credential_ref) = self.credential_ref.as_deref() {
            match credentials::read_secret(credential_ref) {
                Ok(secret) => return Ok(secret),
                // keyring 读不出来的时候才考虑明文：有明文而且用户开了口子就用它，否则
                // 把两件事一起说清楚 —— 读失败的原因，和那份明文为什么没被用。
                Err(error) => {
                    if plaintext.is_empty() {
                        return Err(error);
                    }
                    if plaintext_keys_allowed() {
                        return Ok(self.api_key.clone());
                    }
                    return Err(format!(
                        "Could not read the stored credential for profile '{}' ({}), and its \
                         plaintext api_key in config.json is ignored by default. Re-enter the key \
                         in Settings, or set {}=1 to use the plaintext one.",
                        self.name, error, ALLOW_PLAINTEXT_KEY_ENV
                    ));
                }
            }
        }
        if plaintext.is_empty() {
            return Err(format!(
                "LLM credential is not configured for profile '{}'",
                self.name
            ));
        }
        if plaintext_keys_allowed() {
            return Ok(self.api_key.clone());
        }
        Err(format!(
            "Profile '{}' only has a plaintext api_key in config.json, which is ignored by \
             default. Re-enter the key in Settings so it goes to the OS keyring. If saving it also \
             fails, this machine has no working keyring — set {}=1 to use the plaintext one.",
            self.name, ALLOW_PLAINTEXT_KEY_ENV
        ))
    }

    pub fn masked_api_key(&self) -> String {
        // 实际探测条目是否可读，而不是"有 credentialRef 就当成已保存"。后者会在写入失败时
        // 谎报密钥已存在，用户看到 "Enter to overwrite" 于是留空保存，陷入永远修不好的循环。
        if let Some(credential_ref) = self.credential_ref.as_deref() {
            if let Ok(secret) = credentials::read_secret(credential_ref) {
                return mask_api_key(&secret);
            }
        }
        // 明文那份要说出它**是明文**，而不是和 keyring 里的那份显示成一模一样。这是
        // ROADMAP 凭据那一节里"可见的 plaintext 指示"那一半：用户有权知道密钥存在哪儿。
        if !self.api_key.trim().is_empty() {
            return format!("{} (plaintext in config.json)", mask_api_key(&self.api_key));
        }
        "not configured".to_string()
    }

    /// 该 profile 是否真的有**能用**的密钥
    pub fn has_readable_api_key(&self) -> bool {
        if self
            .credential_ref
            .as_deref()
            .is_some_and(credentials::has_secret)
        {
            return true;
        }
        // 没开口子的明文不算"能用"：算的话设置面板会显示"已配置"，而每次运行都会失败。
        !self.api_key.trim().is_empty() && plaintext_keys_allowed()
    }
}

/// 明文密钥的显式开关。
///
/// 环境变量而不是配置项：配置文件本身就是那份明文所在的地方，把开关也放进去等于让
/// 被质疑的东西自己签字。
pub const ALLOW_PLAINTEXT_KEY_ENV: &str = "AGENT_IDE_ALLOW_PLAINTEXT_KEY";

fn plaintext_keys_allowed() -> bool {
    std::env::var(ALLOW_PLAINTEXT_KEY_ENV)
        .map(|value| {
            let value = value.trim();
            // `no` / `off` 也算关。一个名字里带 ALLOW 的变量被设成 `no` 却表示"允许"，
            // 本身就是个缺陷 —— 这条和 `0` / `false` 是同一个理由。
            !value.is_empty()
                && !["0", "false", "no", "off"]
                    .iter()
                    .any(|off| value.eq_ignore_ascii_case(off))
        })
        .unwrap_or(false)
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
            prompt_micros_per_million: None,
            completion_micros_per_million: None,
            max_run_spend_micros: None,
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
        prompt_micros_per_million: None,
        completion_micros_per_million: None,
        max_run_spend_micros: None,
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
///
/// 这里**不走**明文那道开关。开关管的是"我们拿它去调模型"，而这个按钮是用户看自己机器上
/// 自己那份配置 —— 拒绝显示只会得到一句"Cannot read stored key"，而它明明读到了、还在
/// 上面那行掩码里写着。看得见和不拿去用是两件事。
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
    if let Some(credential_ref) = profile.credential_ref.as_deref() {
        if let Ok(secret) = credentials::read_secret(credential_ref) {
            return Ok(secret);
        }
    }
    if !profile.api_key.trim().is_empty() {
        return Ok(profile.api_key.clone());
    }
    // 两边都没有的时候，把 `api_key()` 那句带着原因的错误交出去，而不是自己再编一句
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

/// 单次运行的价格与金额上限。
///
/// 价格必须两半都配齐才算配置成功：只配输入价的估算会系统性低估花费，用它执行
/// 上限等于给用户一个假的保障。缺配时返回 `None` 价格，`RunUsageMeter` 会把花费
/// 记成"算不出来"并明说上限未执行，而不是当成免费。
pub fn run_spend_cap(
    config: &LlmProfilesConfig,
    profile_id: Option<&str>,
) -> (Option<TokenPricing>, Option<u64>) {
    let selected_id = profile_id.unwrap_or(&config.active_profile_id);
    let Some(profile) = config
        .profiles
        .iter()
        .find(|profile| profile.id == selected_id)
        .or_else(|| config.profiles.first())
    else {
        return (None, None);
    };
    let pricing = match (
        profile.prompt_micros_per_million,
        profile.completion_micros_per_million,
    ) {
        (Some(prompt), Some(completion)) => Some(TokenPricing {
            prompt_micros_per_million: prompt,
            completion_micros_per_million: completion,
        }),
        _ => None,
    };
    // 和 token 上限一致：0 当作"没设置"，否则一个手写的 0 会锁死所有运行
    let cap = profile.max_run_spend_micros.filter(|cap| *cap > 0);
    (pricing, cap)
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
        prompt_micros_per_million: request.prompt_micros_per_million,
        completion_micros_per_million: request.completion_micros_per_million,
        max_run_spend_micros: request.max_run_spend_micros,
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

/// 云端 profile 的默认工具模式。
///
/// 从 `text_protocol` 改成 `native_tools`：内置的只读工作区工具只在原生工具
/// 模式下才会被声明，而默认关着的话，Agent 只能吃运行开始时打包的上下文，
/// 遇到没预选的文件就只能猜 —— 这正是"文本协议默认"在今天的真实代价。
///
/// 之所以现在敢翻默认：`send_chat_request` 会在供应商明确拒绝 `tools` 时摘掉
/// 参数重试一次，不支持工具的端点会降级而不是整次运行失败。本地 profile 仍然
/// 由 `save_profile` 显式写成 `text_protocol`。
fn default_tool_call_mode() -> String {
    "native_tools".to_string()
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
            prompt_micros_per_million: None,
            completion_micros_per_million: None,
            max_run_spend_micros: None,
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

        // 明文那份要带上它存在哪儿。和 keyring 里的显示成一样，用户就无从知道这台机器上
        // 的密钥其实躺在一个固定路径的明文文件里。
        assert_eq!(
            profile.to_response().api_key_masked,
            "sk-1****7890 (plaintext in config.json)"
        );
        assert_eq!(profile.to_response().effective_input_tokens, Some(123392));
        assert_eq!(profile.to_response().tool_call_mode, "native_tools");
    }

    /// 窗口不填就是"不知道"，不猜一个数。
    ///
    /// 曾经这里套一个全局假定 128k。用户一句话点破了它：现在各家主力是 200k 到 1M，一个
    /// 固定常量在多数情况下都偏小，于是这一行开始报假警 —— 说预算只有十几万，而实际有一百万。
    /// 预留输出的默认留着：那是"给回答留多少"的策略值，和模型无关。
    #[test]
    fn an_unknown_window_is_reported_as_unknown_rather_than_guessed() {
        let mut profile = sample_profile();
        profile.max_context_tokens = None;
        profile.reserved_output_tokens = None;
        profile.max_output_tokens = None;

        assert_eq!(profile.effective_input_tokens(), None);

        // 填了窗口就用填的那个，预留输出仍然有默认
        profile.max_context_tokens = Some(1_000_000);
        assert_eq!(
            profile.effective_input_tokens(),
            Some(1_000_000 - 4_096 - 512)
        );

        // 没填预留输出时退回 Max output（那是这个模型真正会从窗口里占掉的部分）
        profile.max_output_tokens = Some(64_000);
        assert_eq!(
            profile.effective_input_tokens(),
            Some(1_000_000 - 64_000 - 512)
        );

        // 预留比窗口还大也不能变成一个巨大的数（saturating）
        profile.reserved_output_tokens = Some(9_999_999);
        assert_eq!(profile.effective_input_tokens(), Some(0));
    }

    /// 只有明文的 profile，默认**不能**用来跑。
    ///
    /// 这是 ROADMAP 凭据那一节的决定：明文兜底让 keyring 的保证形同虚设 —— 一次迁移失败之后，
    /// `config.json` 里那份明文会永久地优先于 keyring 里那份，两种存储模型的坏处一起吃。
    /// 拒绝要说清楚三件事：为什么没用它、去哪儿重新输、以及那个显式开关。
    #[test]
    fn a_plaintext_only_profile_is_refused_unless_it_is_opted_into() {
        let _guard = workspace::env_test_guard();
        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);

        let mut profile = sample_profile();
        profile.credential_ref = None;
        profile.api_key = "sk-plaintext".to_string();

        let error = profile.api_key().unwrap_err();
        assert!(error.contains("plaintext"), "{}", error);
        assert!(error.contains("Settings"), "{}", error);
        assert!(error.contains(ALLOW_PLAINTEXT_KEY_ENV), "{}", error);
        // 设置面板不能显示成"已配置"：显示成已配置而每次运行都失败，是最难自己修的那种状态
        assert!(!profile.has_readable_api_key());

        // 显式开启之后照用，不再抱怨：可接受与否是用户的判断，不知情才是问题
        std::env::set_var(ALLOW_PLAINTEXT_KEY_ENV, "1");
        assert_eq!(profile.api_key().unwrap(), "sk-plaintext");
        assert!(profile.has_readable_api_key());
        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);
    }

    /// keyring 里那份**赢**，明文那份连看都不看。
    ///
    /// 这是整个改动的正题，而上面两条只证明了兜底那条路。少了这一条，把顺序改回"明文优先"
    /// 也照样全绿 —— 因为那两条用的都是一个读不出来的条目。
    #[test]
    fn the_keyring_value_wins_over_a_plaintext_one() {
        let _guard = workspace::env_test_guard();
        let credential_ref = credentials::llm_credential_ref("keyring-wins-test");
        credentials::store_secret(&credential_ref, "sk-from-keyring")
            .expect("test service should be writable");

        let mut profile = sample_profile();
        profile.credential_ref = Some(credential_ref.clone());
        profile.api_key = "sk-plaintext".to_string();

        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);
        assert_eq!(profile.api_key().unwrap(), "sk-from-keyring");
        // 开了明文开关也还是 keyring 那份：开关是兜底的许可，不是优先级
        std::env::set_var(ALLOW_PLAINTEXT_KEY_ENV, "1");
        assert_eq!(profile.api_key().unwrap(), "sk-from-keyring");
        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);
        // 掩码显示的也该是 keyring 那份，不带 plaintext 后缀
        assert_eq!(profile.masked_api_key(), "sk-f****ring");

        let _ = credentials::delete_secret(&credential_ref);
    }

    /// keyring 读不出来时，错误里要**同时**有读失败的原因和那份明文为什么被忽略。
    ///
    /// 只说其中一半的话，用户会去修错的那一边：只说"读不出来"他会重输（而重输也会失败，
    /// 因为写入同样坏了），只说"明文被忽略"他不知道 keyring 出了什么事。
    #[test]
    fn a_failed_keyring_read_says_why_the_plaintext_was_not_used() {
        let _guard = workspace::env_test_guard();
        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);

        let mut profile = sample_profile();
        // 一个几乎不可能存在的条目名，保证读取失败
        profile.credential_ref = Some("llm-profile:missing-on-purpose-9f3a".to_string());
        profile.api_key = "sk-plaintext".to_string();

        let error = profile.api_key().unwrap_err();
        // 读失败的那一半：`credentials::read_secret` 的原话被带了进来
        assert!(error.contains("Credential"), "{}", error);
        // 明文被忽略的那一半
        assert!(error.contains("plaintext"), "{}", error);
        assert!(error.contains(ALLOW_PLAINTEXT_KEY_ENV), "{}", error);
    }

    /// `0` 和 `false` 不算开启：一个名字里带 ALLOW 的变量被设成 `0`，意思是不允许。
    #[test]
    fn the_plaintext_switch_reads_like_a_switch() {
        let _guard = workspace::env_test_guard();
        for value in ["", "0", "false", "FALSE", "no", "off", "Off"] {
            std::env::set_var(ALLOW_PLAINTEXT_KEY_ENV, value);
            assert!(
                !plaintext_keys_allowed(),
                "{:?} should not enable it",
                value
            );
        }
        for value in ["1", "true", "yes"] {
            std::env::set_var(ALLOW_PLAINTEXT_KEY_ENV, value);
            assert!(plaintext_keys_allowed(), "{:?} should enable it", value);
        }
        std::env::remove_var(ALLOW_PLAINTEXT_KEY_ENV);
    }

    fn sample_profile() -> LlmProfile {
        LlmProfile {
            id: "p1".to_string(),
            name: "Work".to_string(),
            provider: "openai".to_string(),
            endpoint: "https://api.openai.com/v1".to_string(),
            credential_ref: None,
            api_key: String::new(),
            model: "gpt-4o".to_string(),
            max_context_tokens: None,
            reserved_output_tokens: None,
            max_output_tokens: None,
            max_run_tokens: None,
            prompt_micros_per_million: None,
            completion_micros_per_million: None,
            max_run_spend_micros: None,
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
        }
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
            prompt_micros_per_million: None,
            completion_micros_per_million: None,
            max_run_spend_micros: None,
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
        assert_eq!(serialized["toolCallMode"], "native_tools");
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

    /// 金额上限要么两半价格都配齐、要么就算没配。只配一半时用它执行上限会系统性
    /// 低估花费，那比不执行更糟——用户以为有保障。
    #[test]
    fn run_spend_cap_needs_both_halves_of_the_price() {
        let full: LlmProfile = serde_json::from_value(serde_json::json!({
            "id": "full",
            "name": "Full",
            "provider": "deepseek",
            "endpoint": "https://api.deepseek.com",
            "model": "deepseek-v4-flash",
            "promptMicrosPerMillion": 280_000,
            "completionMicrosPerMillion": 420_000,
            "maxRunSpendMicros": 500_000
        }))
        .expect("profile");
        let mut half = full.clone();
        half.id = "half".to_string();
        half.completion_micros_per_million = None;
        let mut zeroed = full.clone();
        zeroed.id = "zeroed".to_string();
        zeroed.max_run_spend_micros = Some(0);

        let config = LlmProfilesConfig {
            profiles: vec![full, half, zeroed],
            active_profile_id: "full".to_string(),
            context_compression: ContextCompressionMode::default(),
        };

        assert_eq!(
            run_spend_cap(&config, None),
            (
                Some(TokenPricing {
                    prompt_micros_per_million: 280_000,
                    completion_micros_per_million: 420_000,
                }),
                Some(500_000)
            )
        );
        // 价格只配了一半：上限还在，但价格算不出来，于是不会被执行
        assert_eq!(run_spend_cap(&config, Some("half")).0, None);
        assert_eq!(run_spend_cap(&config, Some("half")).1, Some(500_000));
        // 0 当作没设置，和 token 上限一致
        assert_eq!(run_spend_cap(&config, Some("zeroed")).1, None);
    }

    /// 云端 profile 默认走原生工具，否则内置的工作区读取工具永远不会被声明。
    /// 供应商不支持时由 `send_chat_request` 摘掉参数降级，而不是整次失败。
    #[test]
    fn cloud_profile_deserialization_defaults_to_native_tools() {
        let profile: LlmProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "Work",
            "provider": "openai",
            "endpoint": "https://api.openai.com/v1",
            "model": "gpt-4o"
        }))
        .expect("profile");

        assert_eq!(profile.tool_call_mode, "native_tools");
    }
}
