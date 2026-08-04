use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

/// LLM 配置
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub endpoint: String, // e.g. "https://api.openai.com/v1"
    pub api_key: String,
    pub model: String, // e.g. "gpt-4"
    pub provider: String,
    pub max_output_tokens: Option<u32>,
    pub tool_call_mode: String,
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
        Self { config, client }
    }

    /// 流式 Chat 请求，通过 mpsc::Sender 发送每个 token
    pub async fn stream_chat(
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
                                    let _ = tx.send(text.clone()).await;
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
    let response = if self::is_workflow_mock(&messages) && system.contains("software engineering planner") {
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
    let _ = tx.send(response.clone()).await;
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
