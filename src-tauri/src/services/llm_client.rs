use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::mpsc;

// 引入 async_trait 用于异步 trait
use async_trait::async_trait;

/// 模型类型枚举
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ModelType {
    /// OpenAI 兼容的云端模型
    OpenAI,
    /// DeepSeek 云端模型
    DeepSeek,
    /// StarCoder 本地模型 (Hugging Face)
    StarCoder,
    /// CodeLlama 本地模型 (Meta)
    CodeLlama,
    /// DeepSeek Coder 本地模型
    DeepSeekCoder,
    /// CodeGemma 本地模型 (Google)
    CodeGemma,
    /// 其他自定义模型
    Custom(String),
}

impl ModelType {
    /// 从字符串解析模型类型
    pub fn from_string(s: &str) -> Self {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "openai" | "gpt" => ModelType::OpenAI,
            "deepseek" | "deepseek-chat" | "deepseek-coder" => {
                if lower.contains("coder") {
                    ModelType::DeepSeekCoder
                } else {
                    ModelType::DeepSeek
                }
            }
            "starcoder" | "bigcode" => ModelType::StarCoder,
            "codellama" | "llama" => ModelType::CodeLlama,
            "codegemma" | "gemma" => ModelType::CodeGemma,
            _ => ModelType::Custom(s.to_string()),
        }
    }

    /// 是否为本地模型
    pub fn is_local(&self) -> bool {
        matches!(
            self,
            ModelType::StarCoder
                | ModelType::CodeLlama
                | ModelType::DeepSeekCoder
                | ModelType::CodeGemma
        )
    }

    /// 获取默认的本地模型路径
    pub fn default_model_path(&self) -> Option<String> {
        match self {
            ModelType::StarCoder => Some("~/.agent-ide/models/starcoder".to_string()),
            ModelType::CodeLlama => Some("~/.agent-ide/models/codellama".to_string()),
            ModelType::DeepSeekCoder => Some("~/.agent-ide/models/deepseek-coder".to_string()),
            ModelType::CodeGemma => Some("~/.agent-ide/models/codegemma".to_string()),
            _ => None,
        }
    }
}

impl std::fmt::Display for ModelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ModelType::OpenAI => "openai",
            ModelType::DeepSeek => "deepseek",
            ModelType::StarCoder => "starcoder",
            ModelType::CodeLlama => "codellama",
            ModelType::DeepSeekCoder => "deepseek-coder",
            ModelType::CodeGemma => "codegemma",
            ModelType::Custom(name) => name,
        };
        f.write_str(name)
    }
}

/// 模型能力
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// 最大上下文长度（token数）
    pub max_context_tokens: u32,
    /// 支持的编程语言
    pub supported_languages: Vec<String>,
    /// 推理速度（毫秒/token）
    pub inference_speed_ms: f32,
    /// 内存需求（MB）
    pub memory_requirement_mb: u32,
    /// 是否支持流式输出
    pub supports_streaming: bool,
    /// 是否支持工具调用
    pub supports_tool_calls: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            max_context_tokens: 4096,
            supported_languages: vec![
                "typescript".to_string(),
                "javascript".to_string(),
                "python".to_string(),
                "rust".to_string(),
                "go".to_string(),
            ],
            inference_speed_ms: 50.0,
            memory_requirement_mb: 1024,
            supports_streaming: true,
            supports_tool_calls: false,
        }
    }
}

/// 本地模型引擎接口
#[async_trait]
pub trait ModelEngine: Send + Sync {
    /// Load the model lazily before the first request.
    async fn load_model(&self) -> Result<(), String> {
        Ok(())
    }

    async fn unload_model(&self) {}

    fn is_model_loaded(&self) -> bool {
        false
    }

    /// 生成文本
    async fn generate(&self, prompt: &str) -> Result<String, String>;

    /// 流式生成文本
    async fn generate_stream(
        &self,
        prompt: &str,
        tx: mpsc::Sender<String>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<String, String>;

    /// 获取模型能力
    fn capabilities(&self) -> ModelCapabilities;

    /// 获取模型类型
    fn model_type(&self) -> ModelType;
}

/// 本地模型配置
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalModelConfig {
    /// 模型名称
    pub name: String,
    /// 模型类型
    pub model_type: ModelType,
    /// 模型目录路径
    pub model_path: String,
    /// GGUF 文件名
    #[serde(default)]
    pub model_file: String,
    /// 是否启用
    pub enabled: bool,
    #[serde(default)]
    pub n_threads: i32,
    #[serde(default)]
    pub n_ctx: u32,
    #[serde(default)]
    pub n_gpu_layers: i32,
    #[serde(default)]
    pub n_batch: i32,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub top_p: f32,
    #[serde(default)]
    pub top_k: i32,
    #[serde(default)]
    pub max_tokens: u32,
}

impl LocalModelConfig {
    /// 创建默认配置
    pub fn default_starcoder() -> Self {
        Self {
            name: "StarCoder".to_string(),
            model_type: ModelType::StarCoder,
            model_path: "~/.agent-ide/models/starcoder".to_string(),
            model_file: String::new(),
            enabled: false,
            n_threads: 4,
            n_ctx: 4096,
            n_gpu_layers: 0,
            n_batch: 512,
            temperature: 0.2,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 512,
        }
    }

    /// 创建 CodeLlama 配置
    pub fn default_codellama() -> Self {
        Self {
            name: "CodeLlama".to_string(),
            model_type: ModelType::CodeLlama,
            model_path: "~/.agent-ide/models/codellama".to_string(),
            model_file: String::new(),
            enabled: false,
            n_threads: 4,
            n_ctx: 4096,
            n_gpu_layers: 0,
            n_batch: 512,
            temperature: 0.2,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 512,
        }
    }

    /// 创建 DeepSeek Coder 配置
    pub fn default_deepseek_coder() -> Self {
        Self {
            name: "DeepSeek Coder".to_string(),
            model_type: ModelType::DeepSeekCoder,
            model_path: "~/.agent-ide/models/deepseek-coder".to_string(),
            model_file: String::new(),
            enabled: false,
            n_threads: 4,
            n_ctx: 4096,
            n_gpu_layers: 0,
            n_batch: 512,
            temperature: 0.2,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 512,
        }
    }

    /// 创建 CodeGemma 配置
    pub fn default_codegemma() -> Self {
        Self {
            name: "CodeGemma".to_string(),
            model_type: ModelType::CodeGemma,
            model_path: "~/.agent-ide/models/codegemma".to_string(),
            model_file: String::new(),
            enabled: false,
            n_threads: 4,
            n_ctx: 4096,
            n_gpu_layers: 0,
            n_batch: 512,
            temperature: 0.2,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 512,
        }
    }
}

/// LLM 配置
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub endpoint: String, // e.g. "https://api.openai.com/v1"
    pub api_key: String,
    pub model: String, // e.g. "gpt-4"
    pub provider: String,
    pub max_output_tokens: Option<u32>,
    pub tool_call_mode: String,
    pub model_type: ModelType,
    pub local_model_config: Option<LocalModelConfig>,
}

impl LlmConfig {
    /// 创建 OpenAI 配置
    pub fn openai(api_key: String, model: String) -> Self {
        Self {
            endpoint: "https://api.openai.com/v1".to_string(),
            api_key,
            model,
            provider: "openai".to_string(),
            max_output_tokens: Some(4096),
            tool_call_mode: "native_tools".to_string(),
            model_type: ModelType::OpenAI,
            local_model_config: None,
        }
    }

    /// 创建 DeepSeek 配置
    pub fn deepseek(api_key: String, model: String) -> Self {
        Self {
            endpoint: "https://api.deepseek.com/v1".to_string(),
            api_key,
            model,
            provider: "deepseek".to_string(),
            max_output_tokens: Some(4096),
            tool_call_mode: "native_tools".to_string(),
            model_type: ModelType::DeepSeek,
            local_model_config: None,
        }
    }

    /// 创建本地模型配置
    pub fn local_model(local_config: LocalModelConfig) -> Self {
        let model_type = local_config.model_type.clone();
        Self {
            endpoint: format!("local://{}", local_config.name),
            api_key: String::new(),
            model: local_config.name.clone(),
            provider: "local".to_string(),
            max_output_tokens: Some(local_config.max_tokens.max(1)),
            tool_call_mode: "text_protocol".to_string(),
            model_type,
            local_model_config: Some(local_config),
        }
    }
}

/// Chat 消息。
///
/// `tool_calls` / `tool_call_id` 只在 provider 原生工具调用回合中出现：
/// - assistant 回合：重放模型请求的工具调用，`content` 可为空串。
/// - tool 回合：`role = "tool"`，`tool_call_id` 关联对应调用，`content` 为工具结果。
///
/// 两个字段为 None 时不参与序列化，因此普通请求体与扩展前完全一致。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OutboundToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            ..Default::default()
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            ..Default::default()
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            ..Default::default()
        }
    }

    /// 重放模型在上一回合请求的原生工具调用。
    pub fn assistant_tool_calls(content: impl Into<String>, calls: &[LlmToolCall]) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Some(calls.iter().map(OutboundToolCall::from).collect()),
            tool_call_id: None,
        }
    }

    /// 单个工具的执行结果回传。
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Provider 原生工具调用（由流式/非流式响应重组得到）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 回传给 provider 的工具调用结构（OpenAI 兼容 `assistant.tool_calls[]`）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OutboundToolFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundToolFunction {
    pub name: String,
    pub arguments: String,
}

impl From<&LlmToolCall> for OutboundToolCall {
    fn from(call: &LlmToolCall) -> Self {
        Self {
            id: call.id.clone(),
            call_type: "function".to_string(),
            function: OutboundToolFunction {
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            },
        }
    }
}

/// 注入请求体的额外工具定义（当前来源：MCP server 发现的工具）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema 对象；非对象时回退为空 object schema
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    fn to_request_value(&self) -> serde_json::Value {
        let parameters = if self.parameters.is_object() {
            self.parameters.clone()
        } else {
            serde_json::json!({ "type": "object", "properties": {} })
        };
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": parameters,
            }
        })
    }
}

/// 供应商回报的 token 用量。
///
/// 字段可缺失：不是所有 OpenAI 兼容实现都返回 `usage`，本地 runtime 尤其常常不返回。
/// 因此这里全部是 `Option`，调用方必须能处理"拿不到用量"这种情况，而不是把缺失
/// 当成 0 —— 后者会让基于用量的上限形同虚设。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LlmUsage {
    #[serde(default, rename = "prompt_tokens")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, rename = "completion_tokens")]
    pub completion_tokens: Option<u64>,
    #[serde(default, rename = "total_tokens")]
    pub total_tokens: Option<u64>,
}

impl LlmUsage {
    /// 优先用供应商给的 total，缺失时退回 prompt + completion
    pub fn resolved_total(&self) -> Option<u64> {
        if let Some(total) = self.total_tokens {
            return Some(total);
        }
        match (self.prompt_tokens, self.completion_tokens) {
            (None, None) => None,
            (prompt, completion) => {
                Some(prompt.unwrap_or_default() + completion.unwrap_or_default())
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.prompt_tokens.is_none()
            && self.completion_tokens.is_none()
            && self.total_tokens.is_none()
    }
}

/// 一次运行内的累计 token 用量，以及可选的用量上限。
///
/// 放在 `LlmClient` 上而不是让 `execute_stage` / `execute_step` 把用量层层返回：
/// 一次运行会经过 planner、每个 pipeline stage、以及每个 stage 内最多 5 轮工具回合，
/// 这些路径的返回类型各不相同（planner 走的 `stream_chat` 只返回 `String`）。
/// 挂在客户端上意味着**所有**入口都会被记账，也没法绕过上限。
#[derive(Debug, Default)]
pub struct RunUsageMeter {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    /// 发出去的供应商请求数
    calls: AtomicU64,
    /// 其中供应商回报了用量的请求数
    reported_calls: AtomicU64,
    max_total_tokens: Option<u64>,
}

/// 某一时刻的用量快照
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub calls: u64,
    pub reported_calls: u64,
    pub max_total_tokens: Option<u64>,
}

impl RunUsageSnapshot {
    /// 有请求发出但没有一次回报用量：此时 total 是 0，但那代表"不知道"而不是"没花"
    pub fn usage_is_unknown(&self) -> bool {
        self.calls > 0 && self.reported_calls == 0
    }
}

impl RunUsageMeter {
    pub fn new(max_total_tokens: Option<u64>) -> Self {
        Self {
            max_total_tokens,
            ..Default::default()
        }
    }

    /// 发请求前检查上限。超了就直接拒绝，而不是等这次请求也花完再说。
    pub fn check_budget(&self) -> Result<(), String> {
        let Some(limit) = self.max_total_tokens else {
            return Ok(());
        };
        let snapshot = self.snapshot();
        if snapshot.total_tokens >= limit {
            return Err(format!(
                "Run token cap reached: {} of {} tokens used across {} LLM call(s). Raise the per-run cap or start a new run.",
                snapshot.total_tokens, limit, snapshot.calls
            ));
        }
        Ok(())
    }

    fn record_call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    /// 记录一次调用回报的用量。供应商没报用量时只记调用数，不把缺失当成 0。
    pub fn record_usage(&self, usage: Option<&LlmUsage>) {
        let Some(usage) = usage else {
            return;
        };
        if usage.is_empty() {
            return;
        }
        self.reported_calls.fetch_add(1, Ordering::SeqCst);
        self.prompt_tokens
            .fetch_add(usage.prompt_tokens.unwrap_or_default(), Ordering::SeqCst);
        self.completion_tokens.fetch_add(
            usage.completion_tokens.unwrap_or_default(),
            Ordering::SeqCst,
        );
    }

    pub fn snapshot(&self) -> RunUsageSnapshot {
        let prompt_tokens = self.prompt_tokens.load(Ordering::SeqCst);
        let completion_tokens = self.completion_tokens.load(Ordering::SeqCst);
        RunUsageSnapshot {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            calls: self.calls.load(Ordering::SeqCst),
            reported_calls: self.reported_calls.load(Ordering::SeqCst),
            max_total_tokens: self.max_total_tokens,
        }
    }
}

/// 流式/非流式请求的统一输出：文本内容 + 原生工具调用 + token 用量
#[derive(Clone, Debug, Default)]
pub struct LlmStreamOutput {
    pub content: String,
    pub tool_calls: Vec<LlmToolCall>,
    /// 供应商未回报用量时为 None
    pub usage: Option<LlmUsage>,
}

impl LlmStreamOutput {
    /// 只有文本、拿不到用量的来源（mock、本地 runtime）
    pub fn from_content(content: String) -> Self {
        Self {
            content,
            tool_calls: Vec::new(),
            usage: None,
        }
    }
}

/// `emit_agent_changes` 工具名：与 native_tools_schema 保持一致
pub const NATIVE_CHANGES_TOOL: &str = "emit_agent_changes";

/// 将原生工具调用合成为 ```agent-changes 围栏块，复用现有解析管线。
/// 仅识别 `emit_agent_changes` 且参数为合法 JSON 的调用。
pub fn synthesize_agent_changes_block(tool_calls: &[LlmToolCall]) -> Option<String> {
    let call = tool_calls
        .iter()
        .find(|call| call.name == NATIVE_CHANGES_TOOL)?;
    let parsed: serde_json::Value = serde_json::from_str(call.arguments.trim()).ok()?;
    if parsed
        .get("changes")
        .and_then(|changes| changes.as_array())
        .is_none_or(|changes| changes.is_empty())
    {
        return None;
    }
    Some(format!(
        "\n```agent-changes\n{}\n```\n",
        serde_json::to_string_pretty(&parsed).ok()?
    ))
}

/// 按流式分片重组工具调用的纯逻辑累积器
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    pending: std::collections::BTreeMap<usize, PendingToolCall>,
}

#[derive(Debug, Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn absorb(&mut self, deltas: &[StreamToolCallDelta]) {
        for delta in deltas {
            let entry = self.pending.entry(delta.index).or_default();
            if let Some(id) = delta.id.as_ref() {
                if !id.is_empty() {
                    entry.id.push_str(id);
                }
            }
            if let Some(function) = delta.function.as_ref() {
                if let Some(name) = function.name.as_ref() {
                    entry.name.push_str(name);
                }
                if let Some(arguments) = function.arguments.as_ref() {
                    entry.arguments.push_str(arguments);
                }
            }
        }
    }

    fn finish(self) -> Vec<LlmToolCall> {
        self.pending
            .into_iter()
            .filter(|(_, pending)| !pending.name.is_empty())
            .map(|(_, pending)| LlmToolCall {
                id: pending.id,
                name: pending.name,
                arguments: pending.arguments,
            })
            .collect()
    }
}

/// 流式响应中的工具调用分片（OpenAI 兼容 `delta.tool_calls[]`）
#[derive(Deserialize)]
struct StreamToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamToolCallFunction>,
}

#[derive(Deserialize)]
struct StreamToolCallFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// LLM 客户端
#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    client: Client,
    /// 本地模型引擎（如果是本地模型）
    local_engine: Option<Arc<dyn ModelEngine>>,
    /// 额外注入的工具定义（MCP 发现的工具）
    extra_tools: Vec<ToolDefinition>,
    /// 本次运行的用量记账与上限；未设置时不记账也不限流
    usage_meter: Option<Arc<RunUsageMeter>>,
    /// 供应商明确拒绝过 `tools` 参数：后续请求不再附带，避免每次都白撞一次 400
    tools_rejected: Arc<AtomicBool>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = if config.provider.eq_ignore_ascii_case("deepseek") {
            Client::builder()
                .http1_only()
                .build()
                .unwrap_or_else(|_| Client::new())
        } else {
            Client::new()
        };
        Self {
            config,
            client,
            local_engine: None, // 将在初始化时设置
            extra_tools: Vec::new(),
            usage_meter: None,
            tools_rejected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 这次运行里供应商是否拒绝过 `tools`（即工具能力已被降级掉）
    pub fn tools_were_rejected(&self) -> bool {
        self.tools_rejected.load(Ordering::SeqCst)
    }

    /// 挂上本次运行的用量记账器（同一个 Arc 可以跨 stage / 工具回合共享）
    pub fn with_usage_meter(mut self, meter: Arc<RunUsageMeter>) -> Self {
        self.usage_meter = Some(meter);
        self
    }

    /// 设置本地模型引擎
    pub fn with_local_engine(mut self, engine: Arc<dyn ModelEngine>) -> Self {
        self.local_engine = Some(engine);
        self
    }

    /// 注入额外工具定义，仅在 `native_tools` 模式下参与请求体
    pub fn with_extra_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.extra_tools = tools;
        self
    }

    pub fn extra_tools(&self) -> &[ToolDefinition] {
        &self.extra_tools
    }

    pub fn get_capabilities(&self) -> ModelCapabilities {
        if let Some(engine) = &self.local_engine {
            return engine.capabilities();
        }
        ModelCapabilities {
            max_context_tokens: 128000,
            supported_languages: [
                "typescript",
                "javascript",
                "python",
                "rust",
                "go",
                "java",
                "cpp",
                "c",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            inference_speed_ms: 20.0,
            memory_requirement_mb: 0,
            supports_streaming: true,
            supports_tool_calls: true,
        }
    }

    pub fn get_model_type(&self) -> ModelType {
        if let Some(engine) = &self.local_engine {
            return engine.model_type();
        }
        ModelType::from_string(&self.config.provider)
    }

    pub fn supports_tool_calls(&self) -> bool {
        self.get_capabilities().supports_tool_calls
    }

    /// 流式 Chat 请求，通过 mpsc::Sender 发送每个 token
    pub async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<String, String> {
        Ok(self
            .stream_chat_with_tools(messages, cancel_flag, tx)
            .await?
            .content)
    }

    /// 流式 Chat 请求并保留 provider 原生工具调用
    pub async fn stream_chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<LlmStreamOutput, String> {
        // 检查是否为本地模型
        if self.config.endpoint.starts_with("local://") || self.config.provider == "local" {
            return self.stream_chat_local(messages, cancel_flag, tx).await;
        }

        // 检查是否为 Mock 模式
        if self.config.endpoint.starts_with("mock://") {
            return stream_mock_chat(messages, cancel_flag, tx)
                .await
                .map(LlmStreamOutput::from_content);
        }

        // 云端模型流式请求
        self.stream_chat_cloud(messages, cancel_flag, tx).await
    }

    /// 本地模型流式请求
    async fn stream_chat_local(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<LlmStreamOutput, String> {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        // 构建提示词
        let prompt = build_prompt_from_messages(&messages);

        // Load lazily and reuse the engine across requests.
        if let Some(ref engine) = self.local_engine {
            engine.load_model().await?;
            return engine
                .generate_stream(&prompt, tx, cancel_flag)
                .await
                .map(LlmStreamOutput::from_content);
        }

        // 如果没有配置本地引擎，返回错误
        Err("Local model engine not configured".to_string())
    }

    /// 云端模型流式请求
    async fn stream_chat_cloud(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<LlmStreamOutput, String> {
        if self.config.endpoint.starts_with("mock://") {
            return stream_mock_chat(messages, cancel_flag, tx)
                .await
                .map(LlmStreamOutput::from_content);
        }

        if prefers_non_streaming(&self.config) {
            return self.complete_chat(messages, cancel_flag, tx).await;
        }

        let url = format!(
            "{}/chat/completions",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = build_chat_request(
            &self.config,
            messages,
            true,
            &self.extra_tools,
            !self.tools_were_rejected(),
        );

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        let response = self
            .send_chat_request(&url, &body, cancel_flag.clone())
            .await?;

        let mut full_response = String::new();
        let mut tool_calls = ToolCallAccumulator::default();
        let mut usage: Option<LlmUsage> = None;
        let mut stream = response.bytes_stream();
        let mut sse_buf = String::new();

        #[derive(Deserialize)]
        struct StreamChunk {
            // 用量块的 choices 是空数组，某些实现干脆省略该字段；
            // 没有 default 时整块会解析失败并被静默丢弃，用量也就永远拿不到
            #[serde(default)]
            choices: Vec<StreamChoice>,
            #[serde(default)]
            usage: Option<LlmUsage>,
        }

        #[derive(Deserialize)]
        struct StreamChoice {
            delta: StreamDelta,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct StreamDelta {
            content: Option<String>,
            #[serde(rename = "reasoning_content")]
            reasoning_content: Option<String>,
            #[serde(default)]
            tool_calls: Option<Vec<StreamToolCallDelta>>,
        }

        loop {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("Agent task cancelled".to_string());
            }
            let chunk_result = tokio::select! {
                _ = wait_for_cancel(cancel_flag.clone()) => {
                    return Err("Agent task cancelled".to_string());
                }
                next = stream.next() => next,
            };
            let Some(chunk_result) = chunk_result else {
                break;
            };
            let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
            // 字节追加到缓冲区，防止 SSE 行被 TCP 分片截断
            sse_buf.push_str(&String::from_utf8_lossy(&chunk));

            // 逐完整行解析（兼容 \r\n / \r / \n 各种行尾）
            while let Some(nl) = sse_buf.find(['\n', '\r']) {
                let is_cr = sse_buf.as_bytes()[nl] == b'\r';
                // 提取行内容并 trim \r 和空白
                let line = sse_buf[..nl].trim().trim_end_matches('\r').to_string();
                // 连同行尾符一起丢弃
                sse_buf.drain(..=nl);
                // 跳过剩余的 \n（处理 \r\n 情况）
                if is_cr && sse_buf.starts_with('\n') {
                    sse_buf.drain(..1);
                }

                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<StreamChunk>(json_str) {
                        if let Some(chunk_usage) = parsed.usage {
                            if !chunk_usage.is_empty() {
                                usage = Some(chunk_usage);
                            }
                        }
                        for choice in &parsed.choices {
                            // 仅取 content，跳过 reasoning_content（推理内容）
                            if let Some(ref text) = choice.delta.content {
                                if !text.is_empty() {
                                    if cancel_flag.load(Ordering::SeqCst) {
                                        return Err("Agent task cancelled".to_string());
                                    }
                                    full_response.push_str(text);
                                    tx.send(text.clone())
                                        .await
                                        .map_err(|_| "LLM stream receiver dropped".to_string())?;
                                }
                            }
                            if let Some(ref deltas) = choice.delta.tool_calls {
                                tool_calls.absorb(deltas);
                            }
                        }
                    }
                }
            }
        }

        if let Some(ref meter) = self.usage_meter {
            meter.record_usage(usage.as_ref());
        }
        Ok(LlmStreamOutput {
            content: full_response,
            tool_calls: tool_calls.finish(),
            usage,
        })
    }

    async fn complete_chat(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<LlmStreamOutput, String> {
        let url = format!(
            "{}/chat/completions",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = build_chat_request(
            &self.config,
            messages,
            false,
            &self.extra_tools,
            !self.tools_were_rejected(),
        );

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        let response = self
            .send_chat_request(&url, &body, cancel_flag.clone())
            .await?;

        #[derive(Deserialize)]
        struct CompletionResponse {
            choices: Vec<CompletionChoice>,
            #[serde(default)]
            usage: Option<LlmUsage>,
        }

        #[derive(Deserialize)]
        struct CompletionChoice {
            message: CompletionMessage,
            /// 空响应最关键的一条线索：`length` 说明被输出上限截断了
            #[serde(default)]
            finish_reason: Option<String>,
        }

        #[derive(Deserialize)]
        struct CompletionMessage {
            content: Option<String>,
            /// 推理模型把思考放这里；正文为空时它的长度能说明预算烧在哪了
            #[serde(default)]
            reasoning_content: Option<String>,
            #[serde(default)]
            tool_calls: Option<Vec<CompletionToolCall>>,
        }

        #[derive(Deserialize)]
        struct CompletionToolCall {
            #[serde(default)]
            id: Option<String>,
            function: CompletionToolFunction,
        }

        #[derive(Deserialize)]
        struct CompletionToolFunction {
            name: Option<String>,
            #[serde(default)]
            arguments: Option<String>,
        }

        let decode = response.json::<CompletionResponse>();
        let payload = tokio::select! {
            _ = wait_for_cancel(cancel_flag.clone()) => {
                return Err("Agent task cancelled".to_string());
            }
            result = decode => {
                result.map_err(|error| format!("LLM response decode failed: {}", error))?
            }
        };
        let mut tool_calls: Vec<LlmToolCall> = Vec::new();
        // choices 会被 into_iter 消耗，用量和条数要先取出来
        let payload_usage = payload.usage.filter(|usage| !usage.is_empty());
        let choice_count = payload.choices.len();
        // 失败时唯一能重现问题的东西就是这几个字段，必须在 choices 被消耗前留下来
        let choice_diagnostics = payload
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                format!(
                    "choice {}: finish_reason={}, content_chars={}, reasoning_chars={}, tool_calls={}",
                    index,
                    choice.finish_reason.as_deref().unwrap_or("none"),
                    choice
                        .message
                        .content
                        .as_deref()
                        .map(|text| text.chars().count())
                        .unwrap_or(0),
                    choice
                        .message
                        .reasoning_content
                        .as_deref()
                        .map(|text| text.chars().count())
                        .unwrap_or(0),
                    choice
                        .message
                        .tool_calls
                        .as_ref()
                        .map(|calls| calls.len())
                        .unwrap_or(0),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let content = payload
            .choices
            .into_iter()
            .find_map(|choice| {
                for call in choice.message.tool_calls.unwrap_or_default() {
                    tool_calls.push(LlmToolCall {
                        id: call.id.unwrap_or_default(),
                        name: call.function.name.unwrap_or_default(),
                        arguments: call.function.arguments.unwrap_or_default(),
                    });
                }
                choice.message.content
            })
            .filter(|text| !text.is_empty());

        // 记账必须先做：这次调用花的钱不因为它失败而消失。以前 record_usage 在
        // 下面的错误返回之后，所以一次空响应会让 per-run cap 少算一整次调用 ——
        // 实测日志里的 "only 1 of 2 LLM call(s) reported usage" 就是这么来的，
        // 不是供应商没回报。
        if let Some(ref meter) = self.usage_meter {
            meter.record_usage(payload_usage.as_ref());
        }

        if content.is_none() && tool_calls.is_empty() {
            return Err(format!(
                "LLM response had no message content and no tool calls. {} choice(s) returned [{}]. \
                 finish_reason=length means the output was cut off at max output tokens; a large \
                 reasoning_chars with empty content means the model spent the whole output budget \
                 on reasoning.",
                choice_count,
                if choice_diagnostics.is_empty() {
                    "no choices".to_string()
                } else {
                    choice_diagnostics
                }
            ));
        }

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }
        if let Some(ref text) = content {
            let _ = tx.send(text.clone()).await;
        }
        Ok(LlmStreamOutput {
            content: content.unwrap_or_default(),
            tool_calls,
            usage: payload_usage,
        })
    }

    async fn send_chat_request(
        &self,
        url: &str,
        body: &serde_json::Value,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<reqwest::Response, String> {
        const MAX_ATTEMPTS: usize = 3;

        // 所有真正打到供应商的请求都经过这里，所以上限检查放在这里就绕不过去。
        // 重试不重复计数：计一次逻辑调用。
        if let Some(ref meter) = self.usage_meter {
            meter.check_budget()?;
            meter.record_call();
        }

        let mut body = body.clone();
        let mut dropped: Vec<&'static str> = Vec::new();

        for attempt in 0..MAX_ATTEMPTS {
            let request = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send();

            let response = tokio::select! {
                _ = wait_for_cancel(cancel_flag.clone()) => {
                    return Err("Agent task cancelled".to_string());
                }
                result = request => {
                    result.map_err(|error| format!("LLM request failed: {}", error))?
                }
            };

            if response.status().is_success() {
                return Ok(response);
            }

            let status = response.status();
            let text = response.text().await.unwrap_or_default();

            // 能力协商：供应商明确说不认某个可选参数时，摘掉它再试一次，
            // 而不是让整次运行直接失败。丢掉 tools 会降级成纯文本协议，
            // 所以要记在 client 上，供命令层结束后写进 action log。
            if let Some(parameter) = unsupported_parameter(status, &text) {
                if !dropped.contains(&parameter) && strip_parameter(&mut body, parameter) {
                    dropped.push(parameter);
                    if parameter == "tools" {
                        self.tools_rejected.store(true, Ordering::SeqCst);
                    }
                    continue;
                }
            }

            let retryable = is_retryable_status(status);
            if !retryable || attempt + 1 == MAX_ATTEMPTS {
                return Err(format!("LLM API error {}: {}", status, text));
            }

            let delay = tokio::time::Duration::from_millis(500 * (attempt as u64 + 1));
            tokio::select! {
                _ = wait_for_cancel(cancel_flag.clone()) => {
                    return Err("Agent task cancelled".to_string());
                }
                _ = tokio::time::sleep(delay) => {}
            }
        }

        Err("LLM request failed after retries".to_string())
    }
}

/// 供应商是否明确拒绝了某个可选请求参数。
///
/// 只在客户端错误上判定，并且要求错误正文点名了该参数——避免把别的 400
/// 误判成"不支持 tools"，那样会把功能静默降级掉。
fn unsupported_parameter(status: reqwest::StatusCode, text: &str) -> Option<&'static str> {
    if !matches!(status.as_u16(), 400 | 404 | 422 | 501) {
        return None;
    }
    let lowered = text.to_lowercase();
    if lowered.contains("stream_options") {
        return Some("stream_options");
    }
    if lowered.contains("tool_choice")
        || lowered.contains("\"tools\"")
        || lowered.contains("'tools'")
        || lowered.contains("function call")
        || lowered.contains("function_call")
    {
        return Some("tools");
    }
    None
}

/// 摘掉一个可选参数；`tools` 连带 `tool_choice` 一起摘，留着它必然再被拒
fn strip_parameter(body: &mut serde_json::Value, parameter: &str) -> bool {
    let Some(object) = body.as_object_mut() else {
        return false;
    };
    let mut removed = object.remove(parameter).is_some();
    if parameter == "tools" {
        removed |= object.remove("tool_choice").is_some();
    }
    removed
}

async fn stream_mock_chat(
    messages: Vec<ChatMessage>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("Agent task cancelled".to_string());
    }
    let system = messages
        .iter()
        .find(|message| message.role == "system")
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    let user = messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    let response =
        if self::is_workflow_mock(&messages) && system.contains("software engineering planner") {
            r#"```plan
[STEP] title="Repair workflow smoke file" type="edit"
```"#
                .to_string()
        } else if self::is_workflow_mock(&messages) && system.contains("Coder Agent") {
            [
                "```diff:smoke.txt",
                "<<<<<<< ORIGINAL",
                "broken",
                "=======",
                "fixed",
                ">>>>>>> UPDATED",
                "```",
            ]
            .join("\n")
        } else if self::is_workflow_mock(&messages) && system.contains("Tester Agent") {
            "Workflow smoke repair is testable by rerunning `npm run workflow`.".to_string()
        } else if system.contains("software engineering planner") {
            r#"```plan
[STEP] title="Update smoke.txt" type="edit"
```"#
                .to_string()
        } else if system.contains("Designer Agent") {
            r#"```sdd
---
type: sdd
title: Smoke Design
version: 1
date: 2026-05-28
status: draft
module: smoke
---

# Smoke Design

## Problem
Capture a lightweight design artifact before implementation.

## Goals
- Produce a reviewable SDD draft.

## Acceptance Criteria
- The SDD can be saved under docs/design.
```"#
                .to_string()
        } else if user.contains("Repair iteration") {
            mock_diff_response("changed", "fixed")
        } else {
            mock_diff_response("initial", "changed")
        };
    tx.send(response.clone())
        .await
        .map_err(|_| "LLM stream receiver dropped".to_string())?;
    Ok(response)
}

fn is_workflow_mock(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|message| {
        message
            .content
            .to_ascii_lowercase()
            .contains("workflow smoke")
    })
}

fn mock_diff_response(original: &str, updated: &str) -> String {
    [
        "```diff:smoke.txt",
        "<<<<<<< ORIGINAL",
        original,
        "=======",
        updated,
        ">>>>>>> UPDATED",
        "```",
    ]
    .join("\n")
}

async fn wait_for_cancel(cancel_flag: Arc<AtomicBool>) {
    while !cancel_flag.load(Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

fn build_prompt_from_messages(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();
    for message in messages {
        prompt.push_str(&message.role);
        prompt.push_str(": ");
        prompt.push_str(&message.content);
        prompt.push('\n');
    }
    prompt.push_str("Assistant:");
    prompt
}

fn build_chat_request(
    config: &LlmConfig,
    messages: Vec<ChatMessage>,
    stream: bool,
    extra_tools: &[ToolDefinition],
    include_tools: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(object) = body.as_object_mut() {
        if stream {
            // 没有这个开关，OpenAI 兼容实现不会在流末尾发送用量块
            object.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }
        if let Some(max_output_tokens) = config.max_output_tokens {
            let key = output_token_key(config);
            object.insert(key.to_string(), serde_json::json!(max_output_tokens));
        }
        if config.tool_call_mode == "native_tools" && include_tools {
            object.insert("tools".to_string(), native_tools_schema(extra_tools));
            object.insert("tool_choice".to_string(), serde_json::json!("auto"));
        }
    }
    body
}

fn prefers_non_streaming(config: &LlmConfig) -> bool {
    config.provider.eq_ignore_ascii_case("deepseek")
        && config.model.eq_ignore_ascii_case("deepseek-v4-flash")
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 503)
}

fn native_tools_schema(extra_tools: &[ToolDefinition]) -> serde_json::Value {
    let mut tools = serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "emit_agent_changes",
                "description": "Emit reviewable Agent IDE file changes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "version": { "type": "integer", "enum": [1] },
                        "changes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "type": { "type": "string", "enum": ["edit", "create"] },
                                    "file": { "type": "string" },
                                    "baseHash": { "type": "string" },
                                    "rationale": { "type": "string" },
                                    "content": { "type": "string" },
                                    "hunks": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "original": { "type": "string" },
                                                "updated": { "type": "string" }
                                            },
                                            "required": ["original", "updated"]
                                        }
                                    }
                                },
                                "required": ["type", "file"]
                            }
                        },
                        "findings": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["version", "changes"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "emit_sdd_draft",
                "description": "Emit an SDD Markdown draft artifact.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "slug": { "type": "string" },
                        "markdown": { "type": "string" },
                        "status": { "type": "string" },
                        "reviewFindings": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["title", "markdown"]
                }
            }
        }
    ]);

    if let Some(array) = tools.as_array_mut() {
        for tool in extra_tools {
            array.push(tool.to_request_value());
        }
    }
    tools
}

fn output_token_key(config: &LlmConfig) -> &'static str {
    let provider = config.provider.to_ascii_lowercase();
    let model = config.model.to_ascii_lowercase();
    if provider == "openai"
        && (model.starts_with("o1")
            || model.starts_with("o3")
            || model.starts_with("o4")
            || model.starts_with("gpt-5"))
    {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

/// StarCoder 模型引擎
pub struct StarCoderEngine {
    /// 保留以便真实推理引擎接入时读取模型路径与采样参数
    #[allow(dead_code)]
    config: LocalModelConfig,
    capabilities: ModelCapabilities,
}

impl StarCoderEngine {
    pub fn new(config: LocalModelConfig) -> Self {
        let capabilities = ModelCapabilities {
            max_context_tokens: 8192,
            supported_languages: vec![
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
                "rust".to_string(),
                "go".to_string(),
            ],
            inference_speed_ms: 80.0,
            memory_requirement_mb: 4096,
            supports_streaming: true,
            supports_tool_calls: false,
        };

        Self {
            config,
            capabilities,
        }
    }
}

#[async_trait::async_trait]
impl ModelEngine for StarCoderEngine {
    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        // 这里应该集成实际的 StarCoder 推理引擎
        // 可以使用 llama-cpp-rs 或其他本地推理库
        Err("StarCoder engine not yet implemented. This is a placeholder for future integration with llama-cpp-rs or candle-core.".to_string())
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        tx: mpsc::Sender<String>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<String, String> {
        let _ = (prompt, tx, cancel_flag);
        Err(
            "StarCoder engine is unavailable until a local inference adapter is configured"
                .to_string(),
        )
    }

    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities.clone()
    }

    fn model_type(&self) -> ModelType {
        ModelType::StarCoder
    }
}

/// CodeLlama 模型引擎
pub struct CodeLlamaEngine {
    /// 保留以便真实推理引擎接入时读取模型路径与采样参数
    #[allow(dead_code)]
    config: LocalModelConfig,
    capabilities: ModelCapabilities,
}

impl CodeLlamaEngine {
    pub fn new(config: LocalModelConfig) -> Self {
        let capabilities = ModelCapabilities {
            max_context_tokens: 16384,
            supported_languages: vec![
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
                "rust".to_string(),
                "go".to_string(),
                "php".to_string(),
                "ruby".to_string(),
            ],
            inference_speed_ms: 60.0,
            memory_requirement_mb: 8192,
            supports_streaming: true,
            supports_tool_calls: false,
        };

        Self {
            config,
            capabilities,
        }
    }
}

#[async_trait::async_trait]
impl ModelEngine for CodeLlamaEngine {
    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        Err("CodeLlama engine not yet implemented. This is a placeholder for future integration with llama-cpp-rs.".to_string())
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        tx: mpsc::Sender<String>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<String, String> {
        let _ = (prompt, tx, cancel_flag);
        Err(
            "CodeLlama engine is unavailable until a local inference adapter is configured"
                .to_string(),
        )
    }

    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities.clone()
    }

    fn model_type(&self) -> ModelType {
        ModelType::CodeLlama
    }
}

/// 本地模型不再进程内推理。
///
/// 这里刻意返回错误而不是静默降级：一个配置了本地模型的 profile 如果被悄悄
/// 当成远端调用，用户会看到一堆莫名其妙的 401/404。让它明确说清该怎么配。
pub fn create_local_model_engine(config: LocalModelConfig) -> Result<Arc<dyn ModelEngine>, String> {
    Err(format!(
        "In-process local inference was removed. Serve {} through an OpenAI-compatible endpoint \
         instead (Ollama http://localhost:11434/v1, LM Studio http://localhost:1234/v1, or vLLM) \
         and configure it as a normal profile endpoint plus model name.",
        config.name
    ))
}

/// 增强的 LLM 客户端工厂
pub struct EnhancedLlmClientFactory;

impl EnhancedLlmClientFactory {
    /// 创建云端 LLM 客户端
    pub fn create_cloud_client(config: LlmConfig) -> LlmClient {
        LlmClient::new(config)
    }

    /// 创建本地 LLM 客户端
    pub fn create_local_client(local_config: LocalModelConfig) -> Result<LlmClient, String> {
        let engine = create_local_model_engine(local_config.clone())?;
        let config = LlmConfig::local_model(local_config);

        Ok(LlmClient::new(config).with_local_engine(engine))
    }

    /// 获取所有可用的本地模型配置
    pub fn get_available_local_models() -> Vec<LocalModelConfig> {
        vec![
            LocalModelConfig::default_starcoder(),
            LocalModelConfig::default_codellama(),
            LocalModelConfig::default_deepseek_coder(),
            LocalModelConfig::default_codegemma(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: &str, model: &str, max_output_tokens: Option<u32>) -> LlmConfig {
        LlmConfig {
            endpoint: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            max_output_tokens,
            tool_call_mode: "text_protocol".to_string(),
            model_type: ModelType::from_string(provider),
            local_model_config: None,
        }
    }

    #[test]
    fn chat_request_omits_output_limit_when_unset() {
        let body = build_chat_request(
            &config("openai", "gpt-4o", None),
            Vec::new(),
            true,
            &[],
            true,
        );

        assert_eq!(body["stream"], true);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn chat_request_maps_output_limit_for_openai_compatible_models() {
        let body = build_chat_request(
            &config("deepseek", "deepseek-chat", Some(2048)),
            Vec::new(),
            true,
            &[],
            true,
        );

        assert_eq!(body["max_tokens"], 2048);
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn chat_request_maps_output_limit_for_openai_reasoning_models() {
        let body = build_chat_request(
            &config("openai", "gpt-5", Some(8192)),
            Vec::new(),
            true,
            &[],
            true,
        );

        assert_eq!(body["max_completion_tokens"], 8192);
        assert!(body.get("max_tokens").is_none());
    }

    /// 不带 include_usage 的话 OpenAI 兼容实现不会在流末尾发送用量块，
    /// 任何基于用量的统计和上限都会永远读不到数
    #[test]
    fn streaming_requests_ask_for_usage_and_non_streaming_does_not() {
        let streamed = build_chat_request(
            &config("openai", "gpt-4o", None),
            Vec::new(),
            true,
            &[],
            true,
        );
        assert_eq!(streamed["stream_options"]["include_usage"], true);

        let once = build_chat_request(
            &config("openai", "gpt-4o", None),
            Vec::new(),
            false,
            &[],
            true,
        );
        assert!(once.get("stream_options").is_none());
    }

    #[test]
    fn usage_chunk_without_choices_still_parses() {
        #[derive(Deserialize)]
        struct StreamChunk {
            #[serde(default)]
            choices: Vec<serde_json::Value>,
            #[serde(default)]
            usage: Option<LlmUsage>,
        }

        // 用量块没有 choices 字段；缺少 serde(default) 时整块会解析失败并被丢弃
        let chunk: StreamChunk = serde_json::from_str(
            r#"{"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}"#,
        )
        .unwrap();

        assert!(chunk.choices.is_empty());
        assert_eq!(chunk.usage.unwrap().resolved_total(), Some(18));
    }

    #[test]
    fn resolved_total_falls_back_to_prompt_plus_completion() {
        let split = LlmUsage {
            prompt_tokens: Some(30),
            completion_tokens: Some(12),
            total_tokens: None,
        };
        assert_eq!(split.resolved_total(), Some(42));

        // 供应商完全不报用量时必须是 None，而不是 0 —— 否则上限判断会形同虚设
        assert_eq!(LlmUsage::default().resolved_total(), None);
        assert!(LlmUsage::default().is_empty());
    }

    #[test]
    fn usage_meter_accumulates_across_calls_and_stops_at_the_cap() {
        let meter = RunUsageMeter::new(Some(100));

        assert!(meter.check_budget().is_ok());
        meter.record_call();
        meter.record_usage(Some(&LlmUsage {
            prompt_tokens: Some(40),
            completion_tokens: Some(20),
            total_tokens: Some(60),
        }));

        // 还没到上限：下一次调用要放行
        assert!(meter.check_budget().is_ok());
        meter.record_call();
        meter.record_usage(Some(&LlmUsage {
            prompt_tokens: Some(30),
            completion_tokens: Some(15),
            total_tokens: None,
        }));

        let snapshot = meter.snapshot();
        assert_eq!(snapshot.prompt_tokens, 70);
        assert_eq!(snapshot.completion_tokens, 35);
        assert_eq!(snapshot.total_tokens, 105);
        assert_eq!(snapshot.calls, 2);
        assert_eq!(snapshot.reported_calls, 2);

        let err = meter.check_budget().unwrap_err();
        assert!(err.contains("105 of 100 tokens"), "{}", err);
    }

    /// 供应商不报用量（本地 runtime、mock）时 total 是 0。这不能被当成"没花"，
    /// 否则上限看起来生效、实际永远不会触发 —— 调用方必须能区分这两种情况。
    #[test]
    fn unreported_usage_is_distinguishable_from_zero_usage() {
        let meter = RunUsageMeter::new(Some(10));
        meter.record_call();
        meter.record_usage(None);
        meter.record_usage(Some(&LlmUsage::default()));

        let snapshot = meter.snapshot();
        assert_eq!(snapshot.total_tokens, 0);
        assert_eq!(snapshot.calls, 1);
        assert_eq!(snapshot.reported_calls, 0);
        assert!(snapshot.usage_is_unknown());
        // 用量未知时不会误伤：上限不会凭空触发
        assert!(meter.check_budget().is_ok());

        // 一次都没调用过，就不算"未知"
        assert!(!RunUsageMeter::new(None).snapshot().usage_is_unknown());
    }

    #[test]
    fn usage_meter_without_a_cap_never_blocks() {
        let meter = RunUsageMeter::new(None);
        meter.record_call();
        meter.record_usage(Some(&LlmUsage {
            prompt_tokens: Some(u32::MAX as u64),
            completion_tokens: Some(1),
            total_tokens: None,
        }));

        assert!(meter.check_budget().is_ok());
        assert_eq!(meter.snapshot().max_total_tokens, None);
    }

    /// 供应商不支持 tools 时整次运行不该直接失败：摘掉参数重试一次，
    /// 代价是降级成纯文本协议。判定必须点名参数，否则别的 400 会被误判成
    /// "不支持 tools"，功能被静默降级掉。
    #[test]
    fn unsupported_parameter_detection_requires_the_parameter_to_be_named() {
        use reqwest::StatusCode;

        assert_eq!(
            unsupported_parameter(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"Unsupported parameter: 'tools'"}}"#
            ),
            Some("tools")
        );
        assert_eq!(
            unsupported_parameter(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"model does not support function calling"}}"#
            ),
            Some("tools")
        );
        assert_eq!(
            unsupported_parameter(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"stream_options is not supported"}}"#
            ),
            Some("stream_options")
        );

        // 无关的 400 不能触发降级
        assert_eq!(
            unsupported_parameter(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"context length exceeded"}}"#
            ),
            None
        );
        // 5xx 交给普通重试逻辑处理
        assert_eq!(
            unsupported_parameter(StatusCode::INTERNAL_SERVER_ERROR, "tools exploded"),
            None
        );
    }

    #[test]
    fn stripping_tools_also_removes_tool_choice() {
        let mut cfg = config("openai", "gpt-4o", None);
        cfg.tool_call_mode = "native_tools".to_string();
        let mut body = build_chat_request(&cfg, Vec::new(), true, &[], true);
        assert!(body.get("tools").is_some());
        assert!(body.get("tool_choice").is_some());

        assert!(strip_parameter(&mut body, "tools"));
        assert!(body.get("tools").is_none());
        // 留着 tool_choice 必然被再拒一次
        assert!(body.get("tool_choice").is_none());
        // 已经摘干净了，再摘一次要返回 false，否则重试循环会空转
        assert!(!strip_parameter(&mut body, "tools"));

        // 记住降级结果后，后续请求根本不再附带 tools
        let degraded = build_chat_request(&cfg, Vec::new(), true, &[], false);
        assert!(degraded.get("tools").is_none());
        assert!(degraded.get("tool_choice").is_none());
    }

    #[test]
    fn chat_request_includes_native_tools_when_enabled() {
        let mut cfg = config("openai", "gpt-4o", Some(1024));
        cfg.tool_call_mode = "native_tools".to_string();

        let body = build_chat_request(&cfg, Vec::new(), true, &[], true);

        assert_eq!(body["tool_choice"], "auto");
        assert!(body["tools"]
            .as_array()
            .is_some_and(|items| items.len() == 2));
    }

    #[test]
    fn chat_request_appends_extra_tool_definitions() {
        let mut cfg = config("openai", "gpt-4o", Some(1024));
        cfg.tool_call_mode = "native_tools".to_string();
        let extra = vec![
            ToolDefinition {
                name: "mcp__files__read".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            },
            ToolDefinition {
                name: "mcp__files__broken_schema".to_string(),
                description: "Schema is not an object".to_string(),
                parameters: serde_json::json!("nonsense"),
            },
        ];

        let body = build_chat_request(&cfg, Vec::new(), true, &extra, true);
        let tools = body["tools"].as_array().expect("tools array");

        assert_eq!(tools.len(), 4);
        assert_eq!(tools[2]["type"], "function");
        assert_eq!(tools[2]["function"]["name"], "mcp__files__read");
        assert_eq!(
            tools[2]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );
        // 非对象 schema 回退成空 object，避免 provider 直接 400
        assert_eq!(tools[3]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn tool_round_trip_messages_serialize_openai_shape() {
        let calls = vec![LlmToolCall {
            id: "call_1".to_string(),
            name: "mcp__files__read".to_string(),
            arguments: "{\"path\":\"a.txt\"}".to_string(),
        }];
        let messages = vec![
            ChatMessage::user("read a.txt"),
            ChatMessage::assistant_tool_calls("", &calls),
            ChatMessage::tool_result("call_1", "hello"),
        ];

        let body = build_chat_request(&config("openai", "gpt-4o", None), messages, true, &[], true);
        let serialized = body["messages"].as_array().expect("messages array");

        assert!(serialized[0].get("tool_calls").is_none());
        assert!(serialized[0].get("tool_call_id").is_none());
        assert_eq!(serialized[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(serialized[1]["tool_calls"][0]["type"], "function");
        assert_eq!(
            serialized[1]["tool_calls"][0]["function"]["name"],
            "mcp__files__read"
        );
        assert_eq!(serialized[2]["role"], "tool");
        assert_eq!(serialized[2]["tool_call_id"], "call_1");
        assert_eq!(serialized[2]["content"], "hello");
    }

    #[test]
    fn deepseek_v4_flash_uses_non_streaming_requests() {
        let cfg = config("deepseek", "deepseek-v4-flash", Some(64));
        let body = build_chat_request(&cfg, Vec::new(), false, &[], true);

        assert!(prefers_non_streaming(&cfg));
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn retries_only_transient_llm_statuses() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn tool_call_accumulator_reassembles_stream_fragments() {
        let mut accumulator = ToolCallAccumulator::default();
        // 首个分片携带 id/name，参数随后按序分片到达
        accumulator.absorb(&[StreamToolCallDelta {
            index: 0,
            id: Some("call_1".to_string()),
            function: Some(StreamToolCallFunction {
                name: Some(NATIVE_CHANGES_TOOL.to_string()),
                arguments: Some("{\"version\":1,\"changes\":".to_string()),
            }),
        }]);
        accumulator.absorb(&[
            StreamToolCallDelta {
                index: 0,
                id: None,
                function: Some(StreamToolCallFunction {
                    name: None,
                    arguments: Some("[{\"type\":\"edit\"}]}".to_string()),
                }),
            },
            StreamToolCallDelta {
                index: 1,
                id: Some("call_2".to_string()),
                function: Some(StreamToolCallFunction {
                    name: Some("other_tool".to_string()),
                    arguments: None,
                }),
            },
        ]);

        let calls = accumulator.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, NATIVE_CHANGES_TOOL);
        assert_eq!(
            calls[0].arguments,
            "{\"version\":1,\"changes\":[{\"type\":\"edit\"}]}"
        );
        assert_eq!(calls[1].id, "call_2");
        assert_eq!(calls[1].name, "other_tool");
        assert_eq!(calls[1].arguments, "");
    }

    #[test]
    fn tool_call_accumulator_drops_empty_name_entries() {
        let mut accumulator = ToolCallAccumulator::default();
        accumulator.absorb(&[StreamToolCallDelta {
            index: 0,
            id: Some("call_empty".to_string()),
            function: None,
        }]);

        assert!(accumulator.finish().is_empty());
    }

    #[test]
    fn stream_tool_call_delta_deserializes_minimal_fragments() {
        let delta: StreamToolCallDelta = serde_json::from_str(
            r#"{"index":2,"id":"call_abc","function":{"name":"emit_agent_changes","arguments":"{\"ver"}}"#,
        )
        .unwrap();
        assert_eq!(delta.index, 2);
        assert_eq!(delta.id.as_deref(), Some("call_abc"));
        let function = delta.function.as_ref().unwrap();
        assert_eq!(function.name.as_deref(), Some("emit_agent_changes"));
        assert_eq!(function.arguments.as_deref(), Some("{\"ver"));

        // OpenAI 兼容实现常省略 id/function，仅保留 index
        let tail: StreamToolCallDelta =
            serde_json::from_str(r#"{"index":2,"function":{"arguments":"sion\":1}"}}"#).unwrap();
        assert!(tail.id.is_none());
        let function = tail.function.as_ref().unwrap();
        assert!(function.name.is_none());
        assert_eq!(function.arguments.as_deref(), Some("sion\":1}"));
    }

    #[test]
    fn synthesize_agent_changes_block_from_native_tool_call() {
        let block = synthesize_agent_changes_block(&[LlmToolCall {
            id: "call_1".to_string(),
            name: NATIVE_CHANGES_TOOL.to_string(),
            arguments: r#"{"version":1,"changes":[{"type":"edit","file":"src/main.rs","content":"fn main() {}"}]}"#.to_string(),
        }])
        .expect("native emit_agent_changes call should synthesize a block");

        assert!(block.contains("```agent-changes"));
        assert!(block.contains("\"changes\""));
        assert!(block.contains("src/main.rs"));
    }

    #[test]
    fn synthesize_agent_changes_block_rejects_unusable_calls() {
        let usable = LlmToolCall {
            id: "call_1".to_string(),
            name: NATIVE_CHANGES_TOOL.to_string(),
            arguments: r#"{"version":1,"changes":[{"type":"edit"}]}"#.to_string(),
        };
        let empty_changes = LlmToolCall {
            id: "call_2".to_string(),
            name: NATIVE_CHANGES_TOOL.to_string(),
            arguments: r#"{"version":1,"changes":[]}"#.to_string(),
        };
        let invalid_json = LlmToolCall {
            id: "call_3".to_string(),
            name: NATIVE_CHANGES_TOOL.to_string(),
            arguments: "not-json".to_string(),
        };
        let unknown_tool = LlmToolCall {
            id: "call_4".to_string(),
            name: "other_tool".to_string(),
            arguments: usable.arguments.clone(),
        };

        assert!(synthesize_agent_changes_block(&[empty_changes]).is_none());
        assert!(synthesize_agent_changes_block(&[invalid_json]).is_none());
        assert!(synthesize_agent_changes_block(std::slice::from_ref(&unknown_tool)).is_none());
        assert!(synthesize_agent_changes_block(&[]).is_none());
        assert!(synthesize_agent_changes_block(std::slice::from_ref(&usable)).is_some());
        // 混合列表中即使存在无关调用，也应命中 emit_agent_changes
        assert!(synthesize_agent_changes_block(&[unknown_tool, usable]).is_some());
    }

    #[tokio::test]
    #[ignore = "requires DEEPSEEK_TEST_KEY and network access"]
    async fn deepseek_v4_flash_live_smoke() {
        let api_key = std::env::var("DEEPSEEK_TEST_KEY")
            .expect("DEEPSEEK_TEST_KEY is required for the live smoke test");
        let client = LlmClient::new(LlmConfig {
            endpoint: "https://api.deepseek.com".to_string(),
            api_key,
            model: "deepseek-v4-flash".to_string(),
            provider: "deepseek".to_string(),
            max_output_tokens: Some(64),
            tool_call_mode: "text_protocol".to_string(),
            model_type: ModelType::DeepSeek,
            local_model_config: None,
        });
        let (tx, mut rx) = mpsc::channel(4);
        let response = tokio::time::timeout(
            tokio::time::Duration::from_secs(60),
            client.stream_chat(
                vec![ChatMessage::user("Reply with exactly AGENT_IDE_OK")],
                Arc::new(AtomicBool::new(false)),
                tx,
            ),
        )
        .await
        .expect("DeepSeek request timed out")
        .expect("DeepSeek request failed");

        assert!(!response.trim().is_empty());
        assert_eq!(rx.recv().await.as_deref(), Some(response.as_str()));
    }
}
