//! MCP 命令层：服务器配置管理、工具发现、工具调用，以及 Agent 运行时的工具接线。

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
    app: &AppHandle,
    llm: LlmClient,
    policy: McpToolPolicy,
) -> (LlmClient, Option<Arc<dyn ToolInvoker>>) {
    let definitions = registry.tool_definitions(policy).await;
    if definitions.is_empty() {
        return (llm, None);
    }
    let invoker: Arc<dyn ToolInvoker> = Arc::new(McpToolInvoker {
        registry: registry.clone(),
        app: app.clone(),
        policy,
    });
    (llm.with_extra_tools(definitions), Some(invoker))
}

struct McpToolInvoker {
    registry: Arc<McpRegistry>,
    app: AppHandle,
    policy: McpToolPolicy,
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
        let _ = self.app.emit("agent-action-log", entry);
    }
}

#[async_trait]
impl ToolInvoker for McpToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        is_mcp_tool_name(tool_name)
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        self.log(
            "info",
            &format!("Calling MCP tool {}", tool_name),
            &format!("Arguments:\n{}", truncate(arguments, 2000)),
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
    use super::truncate;

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
