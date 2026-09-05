//! MCP (Model Context Protocol) 客户端：stdio 传输 + 工具发现/调用。
//!
//! 实现说明：MCP stdio 传输就是行分隔的 JSON-RPC 2.0，这里直接手写协议，
//! 不引入 rmcp SDK。理由是当前只需要 `initialize` / `tools/list` / `tools/call`
//! 三个方法，手写实现约 300 行且零新增依赖，避免 SDK 版本与 tokio/serde
//! 约束冲突。若后续需要 resources/prompts/sampling，再评估换成 rmcp。

use crate::services::llm_client::ToolDefinition;
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// 注入 provider 工具列表时的命名前缀，与 Codex/Claude Code 约定保持一致
pub const MCP_TOOL_PREFIX: &str = "mcp__";
const NAME_SEPARATOR: &str = "__";
const REQUEST_TIMEOUT_SECS: u64 = 30;
const PROTOCOL_VERSION: &str = "2025-06-18";
const CONFIG_FILE: &str = "mcp.json";

fn default_true() -> bool {
    true
}

/// 单个 MCP server 的启动配置
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 允许模型自动调用的工具名（server 自己的工具名，不是 `mcp__...` 限定名）。
    /// 未列出的工具只有在策略放宽到 `allow_all` 时才可见。
    #[serde(default)]
    pub auto_approve: Vec<String>,
}

/// 一次 Agent 运行对 MCP 工具的放行策略。
///
/// 这一层是必需的：MCP server 是外部进程，其工具可以读写文件、访问网络、执行命令。
/// 仅凭"用户启用了这个 server"不足以让模型随意调用其中的任意工具。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPolicy {
    /// 完全不向模型暴露 MCP 工具
    Deny,
    /// 只暴露 server 配置里 `autoApprove` 列出的工具
    AutoApprovedOnly,
    /// 暴露已连接 server 的全部工具
    AllowAll,
}

impl McpToolPolicy {
    /// 缺失或无法识别的取值一律回落到最保守的可用策略，避免打错字变成放开全部
    pub fn from_request(value: Option<&str>) -> Self {
        match value {
            Some("allow_all") => Self::AllowAll,
            Some("deny") => Self::Deny,
            _ => Self::AutoApprovedOnly,
        }
    }

    fn permits(&self, tool: &McpToolDescriptor) -> bool {
        match self {
            Self::Deny => false,
            Self::AutoApprovedOnly => tool.auto_approved,
            Self::AllowAll => true,
        }
    }
}

/// 持久化到 `<config_dir>/mcp.json` 的配置
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

fn default_config_version() -> u32 {
    1
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            version: default_config_version(),
            servers: Vec::new(),
        }
    }
}

pub fn config_path() -> std::path::PathBuf {
    workspace::config_dir().join(CONFIG_FILE)
}

pub fn load_config() -> McpConfig {
    let Ok(content) = std::fs::read_to_string(config_path()) else {
        return McpConfig::default();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_config(config: &McpConfig) -> Result<(), String> {
    let dir = workspace::config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Create config dir failed: {}", error))?;
    let json = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Serialize MCP config failed: {}", error))?;
    std::fs::write(config_path(), json)
        .map_err(|error| format!("Write MCP config failed: {}", error))
}

/// 发现到的 MCP 工具
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDescriptor {
    pub server: String,
    pub tool: String,
    /// 注入模型的名字：`mcp__{server}__{tool}`
    pub qualified_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    /// 该工具是否在所属 server 的 `autoApprove` 列表里
    #[serde(default)]
    pub auto_approved: bool,
}

impl McpToolDescriptor {
    pub fn to_tool_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.qualified_name.clone(),
            description: if self.description.trim().is_empty() {
                format!("MCP tool {} from server {}", self.tool, self.server)
            } else {
                format!("[{}] {}", self.server, self.description)
            },
            parameters: self.input_schema.clone(),
        }
    }
}

/// 单个 server 的连接结果
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiscoveryResult {
    pub servers: Vec<McpServerStatus>,
    pub tools: Vec<McpToolDescriptor>,
}

/// 把 server/tool 名规范化成 provider 可接受的工具名。
/// 非 `[A-Za-z0-9_-]` 的字符替换为 `_`，避免注入非法函数名。
fn sanitize_name_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn qualify_tool_name(server: &str, tool: &str) -> String {
    format!(
        "{}{}{}{}",
        MCP_TOOL_PREFIX,
        sanitize_name_part(server),
        NAME_SEPARATOR,
        sanitize_name_part(tool)
    )
}

pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

/// JSON-RPC over stdio 的 MCP 客户端连接
struct McpConnection {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpConnection {
    async fn spawn(config: &McpServerConfig) -> Result<Self, String> {
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // server 的日志走 stderr，直接丢弃避免阻塞管道
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(cwd) = config.cwd.as_ref().filter(|cwd| !cwd.trim().is_empty()) {
            // MCP server 进程的 cwd 仍受 workspace 边界约束
            let resolved = workspace::resolve_existing(cwd)?;
            command.current_dir(workspace::shell_compatible_path(resolved));
        }

        let mut child = command.spawn().map_err(|error| {
            format!(
                "Spawn MCP server '{}' ({}) failed: {}",
                config.name, config.command, error
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("MCP server '{}' has no stdin", config.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("MCP server '{}' has no stdout", config.name))?;

        Ok(Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        })
    }

    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&payload).await?;

        // 跳过通知和不匹配的响应，直到拿到本次请求的 id
        loop {
            let message = self.read_message().await?;
            let Some(response_id) = message.get("id").and_then(serde_json::Value::as_i64) else {
                continue;
            };
            if response_id != id {
                continue;
            }
            if let Some(error) = message.get("error") {
                let message = error
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("MCP {} failed: {}", method, message));
            }
            return Ok(message.get("result").cloned().unwrap_or_default());
        }
    }

    async fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&payload).await
    }

    async fn write_message(&mut self, payload: &serde_json::Value) -> Result<(), String> {
        let mut line = serde_json::to_string(payload)
            .map_err(|error| format!("Serialize MCP request failed: {}", error))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("Write to MCP server failed: {}", error))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("Flush MCP server stdin failed: {}", error))
    }

    async fn read_message(&mut self) -> Result<serde_json::Value, String> {
        loop {
            let mut line = String::new();
            let read = timeout(
                Duration::from_secs(REQUEST_TIMEOUT_SECS),
                self.reader.read_line(&mut line),
            )
            .await
            .map_err(|_| "MCP server response timed out".to_string())?
            .map_err(|error| format!("Read from MCP server failed: {}", error))?;
            if read == 0 {
                return Err("MCP server closed the connection".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // 非 JSON 行通常是 server 误写到 stdout 的日志，忽略而不是让整次调用失败
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return Ok(value);
            }
        }
    }

    async fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "clientInfo": { "name": "agent-ide", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
        .await?;
        self.notify("notifications/initialized", serde_json::json!({}))
            .await
    }

    async fn list_tools(
        &mut self,
        server: &McpServerConfig,
    ) -> Result<Vec<McpToolDescriptor>, String> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        let items = result
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .into_iter()
            .filter_map(|item| {
                let tool = item.get("name")?.as_str()?.to_string();
                Some(McpToolDescriptor {
                    server: server.name.clone(),
                    qualified_name: qualify_tool_name(&server.name, &tool),
                    auto_approved: server.auto_approve.iter().any(|name| name == &tool),
                    tool,
                    description: item
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    input_schema: item
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({ "type": "object" })),
                })
            })
            .collect())
    }

    async fn call_tool(
        &mut self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        let result = self
            .request(
                "tools/call",
                serde_json::json!({ "name": tool, "arguments": arguments }),
            )
            .await?;
        let text = flatten_tool_content(&result);
        if result
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Err(if text.is_empty() {
                format!("MCP tool '{}' reported an error", tool)
            } else {
                text
            });
        }
        Ok(text)
    }

    async fn shutdown(mut self) {
        let _ = self.child.start_kill();
    }
}

/// 把 `tools/call` 的 content 数组压平成回传给模型的文本
fn flatten_tool_content(result: &serde_json::Value) -> String {
    let Some(items) = result.get("content").and_then(serde_json::Value::as_array) else {
        return match result.get("structuredContent") {
            Some(value) => serde_json::to_string(value).unwrap_or_default(),
            None => String::new(),
        };
    };
    let parts: Vec<String> = items
        .iter()
        .map(
            |item| match item.get("type").and_then(serde_json::Value::as_str) {
                Some("text") => item
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                // 图片/音频等非文本内容只回传引用信息，避免把 base64 塞进上下文
                Some(other) => format!("[{} content omitted]", other),
                None => serde_json::to_string(item).unwrap_or_default(),
            },
        )
        .filter(|part| !part.is_empty())
        .collect();
    parts.join("\n")
}

/// 已连接 MCP server 与已发现工具的注册表
#[derive(Default)]
pub struct McpRegistry {
    connections: Mutex<HashMap<String, Arc<Mutex<McpConnection>>>>,
    tools: Mutex<Vec<McpToolDescriptor>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按配置连接所有启用的 server 并刷新工具列表。
    /// 单个 server 失败不影响其他 server，失败原因记录在返回状态里。
    pub async fn discover(&self, config: &McpConfig) -> McpDiscoveryResult {
        self.shutdown_all().await;

        let mut result = McpDiscoveryResult::default();
        for server in config.servers.iter().filter(|server| server.enabled) {
            match Self::connect_and_list(server).await {
                Ok((connection, tools)) => {
                    result.servers.push(McpServerStatus {
                        name: server.name.clone(),
                        connected: true,
                        tool_count: tools.len(),
                        error: None,
                    });
                    result.tools.extend(tools);
                    self.connections
                        .lock()
                        .await
                        .insert(server.name.clone(), Arc::new(Mutex::new(connection)));
                }
                Err(error) => result.servers.push(McpServerStatus {
                    name: server.name.clone(),
                    connected: false,
                    tool_count: 0,
                    error: Some(error),
                }),
            }
        }

        *self.tools.lock().await = result.tools.clone();
        result
    }

    async fn connect_and_list(
        server: &McpServerConfig,
    ) -> Result<(McpConnection, Vec<McpToolDescriptor>), String> {
        let mut connection = McpConnection::spawn(server).await?;
        connection.initialize().await?;
        let tools = connection.list_tools(server).await?;
        Ok((connection, tools))
    }

    pub async fn tools(&self) -> Vec<McpToolDescriptor> {
        self.tools.lock().await.clone()
    }

    /// 按策略过滤后注入模型的工具定义。策略拒绝的工具对模型完全不可见。
    pub async fn tool_definitions(&self, policy: McpToolPolicy) -> Vec<ToolDefinition> {
        self.tools
            .lock()
            .await
            .iter()
            .filter(|tool| policy.permits(tool))
            .map(McpToolDescriptor::to_tool_definition)
            .collect()
    }

    /// 按注入模型的限定名调用工具。`arguments` 是模型生成的 JSON 字符串。
    ///
    /// 策略在这里二次校验，而不是只依赖"没注入模型就不会被调用"：模型可能凭
    /// 历史消息或猜测构造工具名。
    pub async fn call(
        &self,
        qualified_name: &str,
        arguments: &str,
        policy: McpToolPolicy,
    ) -> Result<String, String> {
        let descriptor = self
            .tools
            .lock()
            .await
            .iter()
            .find(|tool| tool.qualified_name == qualified_name)
            .cloned()
            .ok_or_else(|| format!("Unknown MCP tool '{}'", qualified_name))?;

        if !policy.permits(&descriptor) {
            return Err(format!(
                "MCP tool '{}' is not approved for this run. Add it to the '{}' server's auto-approve list or raise the tool approval policy.",
                qualified_name, descriptor.server
            ));
        }

        let connection = self
            .connections
            .lock()
            .await
            .get(&descriptor.server)
            .cloned()
            .ok_or_else(|| format!("MCP server '{}' is not connected", descriptor.server))?;

        let parsed = parse_tool_arguments(arguments)?;
        let mut guard = connection.lock().await;
        guard.call_tool(&descriptor.tool, parsed).await
    }

    pub async fn shutdown_all(&self) {
        let drained: Vec<Arc<Mutex<McpConnection>>> = self
            .connections
            .lock()
            .await
            .drain()
            .map(|(_, c)| c)
            .collect();
        self.tools.lock().await.clear();
        for connection in drained {
            if let Ok(connection) = Arc::try_unwrap(connection) {
                connection.into_inner().shutdown().await;
            }
        }
    }
}

/// 模型给出的参数必须是 JSON 对象；空串按空参数处理
fn parse_tool_arguments(arguments: &str) -> Result<serde_json::Value, String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| format!("Tool arguments are not valid JSON: {}", error))?;
    if !parsed.is_object() {
        return Err("Tool arguments must be a JSON object".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(tool: &str, auto_approved: bool) -> McpToolDescriptor {
        McpToolDescriptor {
            server: "files".to_string(),
            tool: tool.to_string(),
            qualified_name: qualify_tool_name("files", tool),
            description: format!("{} a file", tool),
            input_schema: serde_json::json!({ "type": "object" }),
            auto_approved,
        }
    }

    #[test]
    fn qualified_names_sanitize_unsafe_characters() {
        assert_eq!(
            qualify_tool_name("file system", "read.file"),
            "mcp__file_system__read_file"
        );
        assert!(is_mcp_tool_name("mcp__files__read"));
        assert!(!is_mcp_tool_name("emit_agent_changes"));
    }

    #[test]
    fn unknown_policy_values_fall_back_to_auto_approved_only() {
        assert_eq!(
            McpToolPolicy::from_request(None),
            McpToolPolicy::AutoApprovedOnly
        );
        assert_eq!(
            McpToolPolicy::from_request(Some("")),
            McpToolPolicy::AutoApprovedOnly
        );
        assert_eq!(
            McpToolPolicy::from_request(Some("ALLOW_ALL")),
            McpToolPolicy::AutoApprovedOnly
        );
        assert_eq!(
            McpToolPolicy::from_request(Some("allow_all")),
            McpToolPolicy::AllowAll
        );
        assert_eq!(
            McpToolPolicy::from_request(Some("deny")),
            McpToolPolicy::Deny
        );
    }

    #[test]
    fn policy_gates_tools_by_auto_approval() {
        let approved = descriptor("read", true);
        let unapproved = descriptor("write", false);

        assert!(!McpToolPolicy::Deny.permits(&approved));
        assert!(!McpToolPolicy::Deny.permits(&unapproved));

        assert!(McpToolPolicy::AutoApprovedOnly.permits(&approved));
        assert!(!McpToolPolicy::AutoApprovedOnly.permits(&unapproved));

        assert!(McpToolPolicy::AllowAll.permits(&approved));
        assert!(McpToolPolicy::AllowAll.permits(&unapproved));
    }

    #[test]
    fn tool_definition_prefixes_description_with_server() {
        let definition = descriptor("read", false).to_tool_definition();
        assert_eq!(definition.name, "mcp__files__read");
        assert_eq!(definition.description, "[files] read a file");
    }

    #[test]
    fn tool_definition_falls_back_when_description_missing() {
        let mut blank = descriptor("read", false);
        blank.description = "   ".to_string();

        assert_eq!(
            blank.to_tool_definition().description,
            "MCP tool read from server files"
        );
    }

    #[test]
    fn tool_arguments_must_be_json_objects() {
        assert_eq!(parse_tool_arguments("").unwrap(), serde_json::json!({}));
        assert_eq!(
            parse_tool_arguments("{\"path\":\"a.txt\"}").unwrap(),
            serde_json::json!({ "path": "a.txt" })
        );
        assert!(parse_tool_arguments("[1,2]").is_err());
        assert!(parse_tool_arguments("not json").is_err());
    }

    #[test]
    fn flattens_text_content_and_omits_binary_parts() {
        let result = serde_json::json!({
            "content": [
                { "type": "text", "text": "line one" },
                { "type": "image", "data": "base64..." },
                { "type": "text", "text": "line two" }
            ]
        });

        assert_eq!(
            flatten_tool_content(&result),
            "line one\n[image content omitted]\nline two"
        );
    }

    #[test]
    fn falls_back_to_structured_content_when_no_content_array() {
        let result = serde_json::json!({ "structuredContent": { "ok": true } });
        assert_eq!(flatten_tool_content(&result), "{\"ok\":true}");
        assert_eq!(flatten_tool_content(&serde_json::json!({})), "");
    }

    #[test]
    fn config_round_trips_with_camel_case_fields() {
        let config = McpConfig {
            version: 1,
            servers: vec![McpServerConfig {
                name: "files".to_string(),
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                ],
                env: HashMap::new(),
                cwd: None,
                enabled: true,
                auto_approve: vec!["read_file".to_string()],
            }],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"servers\""));
        assert!(json.contains("\"autoApprove\""));
        let parsed: McpConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.servers.len(), 1);
        assert!(parsed.servers[0].enabled);
        assert_eq!(parsed.servers[0].auto_approve, vec!["read_file"]);
    }

    #[test]
    fn config_defaults_missing_fields() {
        let parsed: McpConfig =
            serde_json::from_str("{\"servers\":[{\"name\":\"a\",\"command\":\"b\"}]}").unwrap();
        assert_eq!(parsed.version, 1);
        assert!(parsed.servers[0].args.is_empty());
        assert!(parsed.servers[0].enabled);
        // 缺省即"没有任何工具被自动批准"
        assert!(parsed.servers[0].auto_approve.is_empty());
    }
}
