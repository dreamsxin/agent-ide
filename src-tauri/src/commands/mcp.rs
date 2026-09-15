//! MCP 命令层：服务器配置管理、工具发现、工具调用，以及 Agent 运行时的工具接线。

use crate::agent::events::RunEvents;
use crate::agent::executor::ToolInvoker;
use crate::agent::orchestrator::ActionLogEntry;
use crate::services::llm_client::LlmClient;
use crate::services::mcp::{
    is_mcp_tool_name, load_config, save_config, McpConfig, McpDiscoveryResult, McpRegistry,
    McpToolDescriptor, McpToolPolicy,
};
use async_trait::async_trait;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// MCP 全局状态：连接与已发现工具的注册表
pub struct McpState {
    pub registry: Arc<McpRegistry>,
}

impl McpState {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(McpRegistry::new()),
        }
    }
}

impl Default for McpState {
    fn default() -> Self {
        Self::new()
    }
}

/// 把 MCP 工具接到一次 Agent 运行上：按策略注入工具定义 + 提供执行器。
///
/// 策略过滤后没有可用工具时返回原样的 client 和 None，Agent 行为与未启用 MCP 时一致。
pub async fn attach_mcp_tools(
    registry: &Arc<McpRegistry>,
    events: Arc<dyn RunEvents>,
    llm: LlmClient,
    policy: McpToolPolicy,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> (LlmClient, Option<Arc<dyn ToolInvoker>>) {
    let definitions = registry.tool_definitions(policy).await;
    if definitions.is_empty() {
        return (llm, None);
    }
    let invoker: Arc<dyn ToolInvoker> = Arc::new(McpToolInvoker {
        registry: registry.clone(),
        events,
        policy,
        cancel,
    });
    (llm.with_extra_tools(definitions), Some(invoker))
}

struct McpToolInvoker {
    registry: Arc<McpRegistry>,
    /// 发事件走 trait，不直接持 `AppHandle`。
    ///
    /// 持 `AppHandle` 的版本在单元测试里根本没法构造，于是"Stop 之后拒绝调用"这条
    /// 分支一行都没有覆盖 —— 和 orchestrator 当初的问题是同一个。
    events: Arc<dyn RunEvents>,
    policy: McpToolPolicy,
    /// 这次运行的副作用开关，和内置工具面、`RunLease` 共用同一个 `Arc`。
    ///
    /// MCP 是本产品最大的副作用面（文件系统、git、HTTP 服务器都可能挂在这里），
    /// 之前它完全不看取消开关：用户点了 Stop，界面变空闲，而排在后面的 MCP 调用
    /// 照旧一个个发出去。
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl McpToolInvoker {
    fn log(&self, level: &str, summary: &str, details: &str) {
        let entry = ActionLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            phase: "mcp_tool_call".to_string(),
            role: None,
            stage: Some("Tool Call".to_string()),
            summary: summary.to_string(),
            details: details.to_string(),
            context_summary: None,
            diff_summary: None,
        };
        self.events
            .emit_json("agent-action-log", serde_json::to_value(entry).unwrap_or_default());
    }
}

#[async_trait]
impl ToolInvoker for McpToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        is_mcp_tool_name(tool_name)
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        // Stop 之后不再往外发调用。MCP 工具做什么我们一概不知道，所以这里只能做能做的
        // 那件事：不开始新的。已经在飞的那一次拦不住 —— 那需要 MCP 客户端支持取消。
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let detail = format!(
                "This run was stopped, so MCP tool {} was not called.",
                tool_name
            );
            self.log("warn", &format!("Refused {} after Stop", tool_name), &detail);
            return Err(detail);
        }
        self.log(
            "info",
            &format!("Calling MCP tool {}", tool_name),
            &format!(
                "Arguments:\n{}",
                truncate(&redact_arguments(arguments), 2000)
            ),
        );
        match self.registry.call(tool_name, arguments, self.policy).await {
            Ok(result) => {
                self.log(
                    "success",
                    &format!("MCP tool {} returned {} chars", tool_name, result.len()),
                    &truncate(&result, 2000),
                );
                Ok(result)
            }
            Err(error) => {
                self.log("error", &format!("MCP tool {} failed", tool_name), &error);
                Err(error)
            }
        }
    }
}

/// 遮蔽工具参数里疑似机密的字段值，再写进 action log。
///
/// 按**键名**判断而不是按值的形态：值形态匹配（比如"看起来像 token 的长字符串"）
/// 既会漏掉短密钥，也会把正常的 hash、id 一起打码。参数不是合法 JSON 时整体
/// 打码 —— 宁可少一条日志，也不要把一段没解析过的文本原样落到日志里。
fn redact_arguments(arguments: &str) -> String {
    const SECRET_KEY_HINTS: [&str; 8] = [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "authorization",
        "credential",
    ];

    fn redact(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, entry) in map.iter_mut() {
                    let normalized = key.to_lowercase().replace(['-', ' '], "_");
                    let looks_secret = SECRET_KEY_HINTS.iter().any(|hint| {
                        normalized.contains(hint) || normalized.replace('_', "") == *hint
                    });
                    if looks_secret && !entry.is_object() && !entry.is_array() {
                        *entry = serde_json::Value::String("[redacted]".to_string());
                    } else {
                        redact(entry);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(redact),
            _ => {}
        }
    }

    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(mut parsed) => {
            redact(&mut parsed);
            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| "[redacted]".to_string())
        }
        Err(_) => "[unparsable arguments redacted]".to_string(),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let head: String = value.chars().take(limit).collect();
    format!("{}\n... [truncated]", head)
}

#[tauri::command]
pub fn get_mcp_config() -> McpConfig {
    load_config()
}

#[tauri::command]
pub fn save_mcp_config(config: McpConfig) -> Result<McpConfig, String> {
    save_config(&config)?;
    Ok(config)
}

/// 重连所有启用的 server 并刷新工具列表。
#[tauri::command]
pub async fn discover_mcp_tools(
    app_handle: AppHandle,
    mcp_state: State<'_, McpState>,
) -> Result<McpDiscoveryResult, String> {
    let config = load_config();
    let result = mcp_state.registry.discover(&config).await;

    let failed: Vec<String> = result
        .servers
        .iter()
        .filter(|server| !server.connected)
        .map(|server| {
            format!(
                "{}: {}",
                server.name,
                server.error.as_deref().unwrap_or("unknown error")
            )
        })
        .collect();
    let entry = ActionLogEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: if failed.is_empty() { "info" } else { "warn" }.to_string(),
        phase: "mcp_discovery".to_string(),
        role: None,
        stage: Some("MCP".to_string()),
        summary: format!(
            "MCP discovery: {} server(s) connected, {} tool(s) available",
            result.servers.iter().filter(|s| s.connected).count(),
            result.tools.len()
        ),
        details: if failed.is_empty() {
            result
                .tools
                .iter()
                .map(|tool| format!("{} ({})", tool.qualified_name, tool.server))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            format!("Failed servers:\n{}", failed.join("\n"))
        },
        context_summary: None,
        diff_summary: None,
    };
    let _ = app_handle.emit("agent-action-log", entry);

    Ok(result)
}

#[tauri::command]
pub async fn get_mcp_tools(
    mcp_state: State<'_, McpState>,
) -> Result<Vec<McpToolDescriptor>, String> {
    Ok(mcp_state.registry.tools().await)
}

/// 手动调用一个 MCP 工具，用于设置面板里验证 server 是否可用。
/// 这是用户主动点击触发的，因此绕过按运行生效的自动批准策略。
#[tauri::command]
pub async fn call_mcp_tool(
    tool_name: String,
    arguments: Option<String>,
    mcp_state: State<'_, McpState>,
) -> Result<String, String> {
    mcp_state
        .registry
        .call(
            &tool_name,
            arguments.as_deref().unwrap_or("{}"),
            McpToolPolicy::AllowAll,
        )
        .await
}

#[tauri::command]
pub async fn disconnect_mcp_servers(mcp_state: State<'_, McpState>) -> Result<(), String> {
    mcp_state.registry.shutdown_all().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{redact_arguments, truncate, McpToolInvoker};
    use crate::agent::events::RecordingEvents;
    use crate::agent::executor::ToolInvoker;
    use crate::services::mcp::{McpRegistry, McpToolPolicy};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Stop 之后不再往外发 MCP 调用，而且这次拒绝要留在操作日志里。
    ///
    /// 这条分支之前一行都没覆盖：执行器持的是 `AppHandle`，单元测试构造不出来。
    /// 现在发事件走 `RunEvents`，测试传 `RecordingEvents` 就能断言。MCP 是本产品最大的
    /// 副作用面，"停了还在发调用"是这里最贵的失败。
    #[test]
    fn a_stopped_run_refuses_further_mcp_calls_and_says_so() {
        let events = Arc::new(RecordingEvents::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let invoker = McpToolInvoker {
            registry: Arc::new(McpRegistry::new()),
            events: events.clone(),
            policy: McpToolPolicy::AllowAll,
            cancel: cancel.clone(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();

        cancel.store(true, Ordering::Relaxed);
        let error = runtime
            .block_on(invoker.invoke("mcp__fs__write_file", "{\"path\":\"a.txt\"}"))
            .unwrap_err();

        assert!(error.contains("stopped"), "{}", error);
        // 拦下来的调用也要能被复盘：只返回错误的话，这件事随对话一起消失
        let logged = events.payloads_for("agent-action-log");
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0]["level"], "warn");
        assert!(
            logged[0]["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("mcp__fs__write_file"),
            "{:?}",
            logged[0]
        );
    }

    /// 没被取消时闸门不挡路：拒绝要来自策略或注册表，不能来自开关。
    #[test]
    fn an_active_run_is_not_blocked_by_the_switch() {
        let events = Arc::new(RecordingEvents::new());
        let invoker = McpToolInvoker {
            registry: Arc::new(McpRegistry::new()),
            events: events.clone(),
            policy: McpToolPolicy::AllowAll,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();

        // 注册表是空的，所以这次调用注定失败 —— 但失败的理由必须是"找不到工具"，
        // 而不是"运行被停了"
        let error = runtime
            .block_on(invoker.invoke("mcp__fs__write_file", "{}"))
            .unwrap_err();
        assert!(!error.contains("stopped"), "{}", error);
        // 正常路径会先记一条"正在调用"
        assert_eq!(events.payloads_for("agent-action-log")[0]["level"], "info");
    }

    /// MCP 工具参数会进 action log。模型把密钥当参数传进来时，日志不能原样留存。
    #[test]
    fn secret_looking_argument_keys_are_redacted() {
        let logged = redact_arguments(
            r#"{"path":"src/app.ts","apiKey":"sk-live-deadbeef","nested":{"Auth-Token":"t0k3n","count":3}}"#,
        );

        assert!(!logged.contains("sk-live-deadbeef"), "{}", logged);
        assert!(!logged.contains("t0k3n"), "{}", logged);
        // 非机密字段照常保留，否则日志失去排查价值
        assert!(logged.contains("src/app.ts"));
        assert!(logged.contains("\"count\": 3"));
    }

    #[test]
    fn unparsable_arguments_are_not_logged_verbatim() {
        let logged = redact_arguments("apiKey=sk-live-deadbeef");

        assert!(!logged.contains("sk-live-deadbeef"));
        assert!(logged.contains("redacted"));
    }

    #[test]
    fn truncate_keeps_short_values_untouched() {
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn truncate_marks_long_values() {
        let truncated = truncate("abcdefghij", 4);
        assert!(truncated.starts_with("abcd"));
        assert!(truncated.ends_with("[truncated]"));
    }
}
