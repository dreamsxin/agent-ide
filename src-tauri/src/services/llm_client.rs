use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
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

    /// 转换为字符串
    pub fn to_string(&self) -> String {
        match self {
            ModelType::OpenAI => "openai".to_string(),
            ModelType::DeepSeek => "deepseek".to_string(),
            ModelType::StarCoder => "starcoder".to_string(),
            ModelType::CodeLlama => "codellama".to_string(),
            ModelType::DeepSeekCoder => "deepseek-coder".to_string(),
            ModelType::CodeGemma => "codegemma".to_string(),
            ModelType::Custom(name) => name.clone(),
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

/// Chat 消息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// LLM 客户端
#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    client: Client,
    /// 本地模型引擎（如果是本地模型）
    local_engine: Option<Arc<dyn ModelEngine>>,
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
        }
    }

    /// 设置本地模型引擎
    pub fn with_local_engine(mut self, engine: Arc<dyn ModelEngine>) -> Self {
        self.local_engine = Some(engine);
        self
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
        // 检查是否为本地模型
        if self.config.endpoint.starts_with("local://") || self.config.provider == "local" {
            return self.stream_chat_local(messages, cancel_flag, tx).await;
        }

        // 检查是否为 Mock 模式
        if self.config.endpoint.starts_with("mock://") {
            return stream_mock_chat(messages, cancel_flag, tx).await;
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
    ) -> Result<String, String> {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        // 构建提示词
        let prompt = build_prompt_from_messages(&messages);

        // Load lazily and reuse the engine across requests.
        if let Some(ref engine) = self.local_engine {
            engine.load_model().await?;
            return engine.generate_stream(&prompt, tx, cancel_flag).await;
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
    ) -> Result<String, String> {
        if self.config.endpoint.starts_with("mock://") {
            return stream_mock_chat(messages, cancel_flag, tx).await;
        }

        if prefers_non_streaming(&self.config) {
            return self.complete_chat(messages, cancel_flag, tx).await;
        }

        let url = format!(
            "{}/chat/completions",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = build_chat_request(&self.config, messages, true);

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        let response = self
            .send_chat_request(&url, &body, cancel_flag.clone())
            .await?;

        let mut full_response = String::new();
        let mut stream = response.bytes_stream();
        let mut sse_buf = String::new();

        #[derive(Deserialize)]
        struct StreamChunk {
            choices: Vec<StreamChoice>,
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
            while let Some(nl) = sse_buf.find(|c| c == '\n' || c == '\r') {
                let is_cr = sse_buf.as_bytes()[nl] == b'\r';
                // 提取行内容并 trim \r 和空白
                let line = sse_buf[..nl].trim().trim_end_matches('\r').to_string();
                // drain: \n case drain through \n; \r case drain through \r
                let drain_end = if is_cr { nl } else { nl };
                sse_buf.drain(..=drain_end);
                // 跳过剩余的 \n（处理 \r\n 情况）
                if is_cr && sse_buf.starts_with('\n') {
                    sse_buf.drain(..1);
                }

                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<StreamChunk>(json_str) {
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
                        }
                    }
                }
            }
        }

        Ok(full_response)
    }

    async fn complete_chat(
        &self,
        messages: Vec<ChatMessage>,
        cancel_flag: Arc<AtomicBool>,
        tx: mpsc::Sender<String>,
    ) -> Result<String, String> {
        let url = format!(
            "{}/chat/completions",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = build_chat_request(&self.config, messages, false);

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }

        let response = self
            .send_chat_request(&url, &body, cancel_flag.clone())
            .await?;

        #[derive(Deserialize)]
        struct CompletionResponse {
            choices: Vec<CompletionChoice>,
        }

        #[derive(Deserialize)]
        struct CompletionChoice {
            message: CompletionMessage,
        }

        #[derive(Deserialize)]
        struct CompletionMessage {
            content: Option<String>,
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
        let content = payload
            .choices
            .into_iter()
            .find_map(|choice| choice.message.content)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "LLM response did not contain message content".to_string())?;

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Agent task cancelled".to_string());
        }
        let _ = tx.send(content.clone()).await;
        Ok(content)
    }

    async fn send_chat_request(
        &self,
        url: &str,
        body: &serde_json::Value,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<reqwest::Response, String> {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 0..MAX_ATTEMPTS {
            let request = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(body)
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
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(object) = body.as_object_mut() {
        if let Some(max_output_tokens) = config.max_output_tokens {
            let key = output_token_key(config);
            object.insert(key.to_string(), serde_json::json!(max_output_tokens));
        }
        if config.tool_call_mode == "native_tools" {
            object.insert("tools".to_string(), native_tools_schema());
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

fn native_tools_schema() -> serde_json::Value {
    serde_json::json!([
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
    ])
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

/// 创建本地模型引擎
pub fn create_local_model_engine(config: LocalModelConfig) -> Result<Arc<dyn ModelEngine>, String> {
    if !config.model_type.is_local() {
        return Err(format!(
            "Unsupported local model type: {:?}",
            config.model_type
        ));
    }
    let inference_config = crate::agent::local_inference::LocalInferenceConfig {
        model_path: std::path::PathBuf::from(config.model_path),
        model_type: config.model_type,
        model_file: config.model_file,
        n_threads: config.n_threads,
        n_ctx: config.n_ctx,
        n_gpu_layers: config.n_gpu_layers,
        n_batch: config.n_batch,
        temperature: config.temperature,
        top_p: config.top_p,
        top_k: config.top_k,
        max_tokens: config.max_tokens,
    };
    Ok(Arc::new(
        crate::agent::local_inference::LlamaCppEngine::new(inference_config)?,
    ))
}

/// 任务上下文
#[derive(Clone, Debug)]
pub struct TaskContext {
    /// 任务复杂度
    pub complexity: Complexity,
    /// 目标语言
    pub language: Option<String>,
    /// 预期输出长度
    pub expected_length: Option<usize>,
}

/// 任务复杂度
#[derive(Clone, Debug, PartialEq)]
pub enum Complexity {
    /// 低复杂度（简单补全）
    Low,
    /// 中等复杂度（代码生成）
    Medium,
    /// 高复杂度（复杂重构）
    High,
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
        let body = build_chat_request(&config("openai", "gpt-4o", None), Vec::new(), true);

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
        );

        assert_eq!(body["max_tokens"], 2048);
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn chat_request_maps_output_limit_for_openai_reasoning_models() {
        let body = build_chat_request(&config("openai", "gpt-5", Some(8192)), Vec::new(), true);

        assert_eq!(body["max_completion_tokens"], 8192);
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn chat_request_includes_native_tools_when_enabled() {
        let mut cfg = config("openai", "gpt-4o", Some(1024));
        cfg.tool_call_mode = "native_tools".to_string();

        let body = build_chat_request(&cfg, Vec::new(), true);

        assert_eq!(body["tool_choice"], "auto");
        assert!(body["tools"]
            .as_array()
            .is_some_and(|items| items.len() == 2));
    }

    #[test]
    fn deepseek_v4_flash_uses_non_streaming_requests() {
        let cfg = config("deepseek", "deepseek-v4-flash", Some(64));
        let body = build_chat_request(&cfg, Vec::new(), false);

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
        });
        let (tx, mut rx) = mpsc::channel(4);
        let response = tokio::time::timeout(
            tokio::time::Duration::from_secs(60),
            client.stream_chat(
                vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Reply with exactly AGENT_IDE_OK".to_string(),
                }],
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
