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

/// 单次 MCP 工具调用回灌模型的最大字符数。
///
/// MCP server 是外部进程，返回体大小完全不受我们控制：一个 `read_file` 工具
/// 可以吐出整个 lockfile，一个 `search` 工具可以吐出上万行匹配。内置 workspace
/// 工具有 `MAX_READ_BYTES` 之类的上限，MCP 路径此前没有对等物，等于把上下文
/// 长度和 token 账单的控制权交给了第三方 server。
pub const MAX_TOOL_RESULT_CHARS: usize = 64_000;

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
        let text = cap_tool_result(&flatten_tool_content(&result));
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

/// 把 MCP 工具返回体截到上限内，并明确告知模型被截断了。
///
/// 保留头部而不是尾部：MCP 工具语义未知，最常见的是"读文件/列目录/查询"，
/// 开头信息量最大。这和 `services::verification::truncate_for_prompt` 故意保留
/// 尾部是不同场景 —— 那里截的是命令输出，报错在最后。
fn cap_tool_result(value: &str) -> String {
    let total = value.chars().count();
    if total <= MAX_TOOL_RESULT_CHARS {
        return value.to_string();
    }
    let head: String = value.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    format!(
        "{}\n... MCP tool result truncated: {} of {} character(s) omitted. Narrow the tool arguments if you need the rest ...",
        head,
        total - MAX_TOOL_RESULT_CHARS,
        total
    )
}

/// 参数里**可能是文件路径**的那几个键名。
///
/// 只认一张固定的表，不去嗅探所有字符串值：MCP 工具的参数里塞的是 URL、正则、整段代码，
/// 逐个字符串猜路径会把记录灌成噪音 —— 而噪音等于没人读，那条记录也就白留了。参考实现
/// 在做"按参数收窄授权"时用的也是一张固定键表，同一个理由。
///
/// 表不全是已知的、可接受的代价：漏掉一个键只意味着这次调用少记一条路径，而记录本身
/// （调了哪个工具、返回多大）仍然在。宁可少说，不可乱说。
const PATH_ARGUMENT_KEYS: [&str; 12] = [
    "path",
    "paths",
    "file",
    "files",
    "file_path",
    "filePath",
    "filepath",
    "directory",
    "dir",
    "cwd",
    "source_path",
    "destination_path",
];

/// 一次调用最多记几条路径。记录是给人读的，十几条路径的一行字没人会读完。
const MAX_RECORDED_PATHS: usize = 8;
/// 单条路径最多记多少字符
const MAX_RECORDED_PATH_CHARS: usize = 200;

/// 一次 MCP 调用的参数里提到的路径，按工作区边界分成两堆。
///
/// 只是**事后描述**，不是授权判断：MCP 工具的参数结构完全由外部 server 定义，凭一张键表
/// 去拦调用，拦掉的多半是用户自己配好的正常用法（比如一个专门管别处目录的 server），
/// 而真想绕开的人换个键名就过去了。所以这里不拒绝，只把"它说要动哪里"记下来 ——
/// 撤不回来的事至少要看得见。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArgumentPaths {
    pub inside: Vec<String>,
    pub outside: Vec<String>,
}

impl ArgumentPaths {
    pub fn is_empty(&self) -> bool {
        self.inside.is_empty() && self.outside.is_empty()
    }

    /// 写进记录的那一句。没提到路径时返回 `None` —— 那种调用（查询、计算）不该被描述成动过文件。
    pub fn describe(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if !self.inside.is_empty() {
            parts.push(format!("inside the workspace: {}", self.inside.join(", ")));
        }
        if !self.outside.is_empty() {
            // 先说外面那一堆：它是这句话里唯一可能让人想撤销的部分
            parts.push(format!(
                "**outside the workspace**: {}",
                self.outside.join(", ")
            ));
        }
        Some(format!(
            "Paths named in its arguments — {}",
            parts.join("; ")
        ))
    }
}

/// 从一次调用的参数里挑出它提到的路径，并按工作区根目录分开。
///
/// 判断是**纯词法**的，不碰磁盘：调用已经发生了，这里只是在描述它说过什么，而一次
/// `canonicalize` 在这个位置既救不回已经写下去的字节，又会因为文件不存在而失败。
pub fn argument_paths(arguments: &str, root: &std::path::Path) -> ArgumentPaths {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return ArgumentPaths::default();
    };
    let mut mentions = Vec::new();
    collect_path_mentions(&value, &mut mentions);

    let root = normalize_lexically(root);
    let mut paths = ArgumentPaths::default();
    for mention in mentions {
        let bucket = if lexically_inside(&root, &mention) {
            &mut paths.inside
        } else {
            &mut paths.outside
        };
        let shown: String = mention.chars().take(MAX_RECORDED_PATH_CHARS).collect();
        if !bucket.contains(&shown) {
            bucket.push(shown);
        }
    }
    paths.inside.truncate(MAX_RECORDED_PATHS);
    paths.outside.truncate(MAX_RECORDED_PATHS);
    paths
}

/// 递归收集路径键下面的字符串值（含字符串数组）。
fn collect_path_mentions(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let is_path_key = PATH_ARGUMENT_KEYS
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(key));
                match child {
                    serde_json::Value::String(text) if is_path_key => push_mention(text, out),
                    serde_json::Value::Array(items) if is_path_key => {
                        for item in items {
                            if let serde_json::Value::String(text) = item {
                                push_mention(text, out);
                            }
                        }
                    }
                    // 嵌套结构要往下走：`{"edits":[{"path":"a.ts"}]}` 这种形状很常见
                    other => collect_path_mentions(other, out),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_path_mentions(item, out);
            }
        }
        _ => {}
    }
}

/// 把一个候选值收进来，顺手挡掉明显不是本地路径的东西。
fn push_mention(text: &str, out: &mut Vec<String>) {
    let trimmed = text.trim();
    if trimmed.is_empty() || out.len() >= MAX_RECORDED_PATHS * 2 {
        return;
    }
    // `http://`、`file://` 之类不是本地路径；单独判 `://` 而不是列协议表，因为要挡的是
    // "它根本不是路径"这一类，而协议名无穷多
    if trimmed.contains("://") {
        return;
    }
    out.push(trimmed.to_string());
}

/// 逐段归约 `.` 和 `..`，不查磁盘。
fn normalize_lexically(path: &std::path::Path) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // 弹不动时保留 `..`：那意味着它确实爬到了起点之上，下面的前缀比较会判成外面
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// 这个路径（相对的按工作区根拼）落在工作区里吗。
fn lexically_inside(root: &std::path::Path, mention: &str) -> bool {
    let candidate = std::path::Path::new(mention);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let normalized = normalize_lexically(&joined);
    // Windows 上大小写不敏感，而 `C:\repo-old` 不能因为前缀像就算进 `C:\repo`，
    // 所以比较按**路径段**走，不按字符串前缀
    let root_parts: Vec<String> = root
        .components()
        .map(|part| comparable_component(&part))
        .collect();
    let mut mention_parts = normalized
        .components()
        .map(|part| comparable_component(&part));
    for expected in root_parts {
        match mention_parts.next() {
            Some(actual) if actual == expected => {}
            _ => return false,
        }
    }
    true
}

fn comparable_component(component: &std::path::Component<'_>) -> String {
    let raw = component.as_os_str().to_string_lossy().to_string();
    if cfg!(windows) {
        raw.to_ascii_lowercase()
    } else {
        raw
    }
}

/// 参数提到、且落在工作区里的那些路径，解析成绝对路径。
///
/// 和 `argument_paths` 共用同一个采集器和同一个边界判断，只是产物不同：那个是给人读的
/// 一句话，这个是"调用之前该给哪几个文件留一份底"的清单。两处各写一份采集逻辑的话，
/// 记录里说的路径和真正留了底的文件迟早不是同一批。
pub fn argument_targets(arguments: &str, root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return Vec::new();
    };
    let mut mentions = Vec::new();
    collect_path_mentions(&value, &mut mentions);

    let root = normalize_lexically(root);
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for mention in mentions {
        if !lexically_inside(&root, &mention) {
            continue;
        }
        let candidate = std::path::Path::new(&mention);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        let resolved = normalize_lexically(&joined);
        if !targets.contains(&resolved) {
            targets.push(resolved);
        }
    }
    targets.truncate(MAX_RECORDED_PATHS);
    targets
}

/// 丢弃与已注册工具限定名冲突的工具，返回 (保留的工具, 冲突说明)。
///
/// 冲突真实存在：`sanitize_name_part` 把非字母数字统一换成 `_`，所以
/// server `a-b` 的 `x` 与 server `a_b` 的 `x` 会得到同一个 `mcp__a_b__x`；
/// 分隔符本身也是 `__`，server `p__q` + tool `r` 撞上 server `p` + tool `q__r`。
/// `McpRegistry::call` 用 `find` 取第一个匹配项，若不处理，模型以为在调 A
/// 实际调到了 B —— 跨 server 静默错投比直接拒绝危险得多。
fn drop_conflicting_tools(
    tools: Vec<McpToolDescriptor>,
    claimed: &mut HashMap<String, String>,
) -> (Vec<McpToolDescriptor>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();
    for tool in tools {
        match claimed.get(&tool.qualified_name) {
            Some(owner) => conflicts.push(format!(
                "tool '{}' maps to '{}', already claimed by server '{}'",
                tool.tool, tool.qualified_name, owner
            )),
            None => {
                claimed.insert(tool.qualified_name.clone(), tool.server.clone());
                accepted.push(tool);
            }
        }
    }
    (accepted, conflicts)
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
        let mut claimed: HashMap<String, String> = HashMap::new();
        for server in config.servers.iter().filter(|server| server.enabled) {
            match Self::connect_and_list(server).await {
                Ok((connection, tools)) => {
                    let (tools, conflicts) = drop_conflicting_tools(tools, &mut claimed);
                    result.servers.push(McpServerStatus {
                        name: server.name.clone(),
                        connected: true,
                        tool_count: tools.len(),
                        error: if conflicts.is_empty() {
                            None
                        } else {
                            Some(format!(
                                "Skipped {} name conflict(s): {}",
                                conflicts.len(),
                                conflicts.join("; ")
                            ))
                        },
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

    /// 把配置里已经不该再活着的 server 停掉，并忘掉它们的工具；返回被停掉的名字。
    ///
    /// 保存配置的时候调用。在面板里把一个 server 关掉或者删掉之后，它的子进程原来还在跑、
    /// 工具还留在注册表里，直到用户再点一次 Discover Tools —— 而删掉最后一个 server 之后
    /// 那个按钮是禁用的，也就是说除了重启应用没有别的办法停掉它。
    ///
    /// 只动"不该再活着"的那些，而不是像 `discover` 一样全停再全起：保存一次配置不该顺带
    /// 重连每一个 server，重连会把本次运行已经发现的工具列表整个换掉。
    pub async fn retain_configured(&self, config: &McpConfig) -> Vec<String> {
        let connected: Vec<String> = self.connections.lock().await.keys().cloned().collect();
        let dropped = servers_to_drop(&connected, config);
        if dropped.is_empty() {
            return dropped;
        }
        let mut removed: Vec<Arc<Mutex<McpConnection>>> = Vec::new();
        {
            let mut guard = self.connections.lock().await;
            for name in &dropped {
                if let Some(connection) = guard.remove(name) {
                    removed.push(connection);
                }
            }
        }
        self.tools
            .lock()
            .await
            .retain(|tool| !dropped.contains(&tool.server));
        for connection in removed {
            if let Ok(connection) = Arc::try_unwrap(connection) {
                connection.into_inner().shutdown().await;
            }
        }
        dropped
    }
}

/// 当前连着的 server 里，哪些在新配置下不该再活着。
///
/// 抽成纯函数是为了能断言它：真正的连接需要拉起子进程，所以"停错了哪个"在测试里唯一看得见
/// 的方式就是这一层。被停掉的判定有两种 —— 配置里删掉了，或者留着但 `enabled` 关掉了；
/// 只看前一种的话，"关掉开关"就变成了一个什么都没发生的开关。
fn servers_to_drop(connected: &[String], config: &McpConfig) -> Vec<String> {
    connected
        .iter()
        .filter(|name| {
            !config
                .servers
                .iter()
                .any(|server| server.enabled && &&server.name == name)
        })
        .cloned()
        .collect()
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

    /// 保存配置之后，该停的 server 必须被算进去；不该停的一个都不能动。
    ///
    /// 这条盯的是一个"开关什么都没做"的缺陷：把 server 的 `enabled` 关掉，原来只是写了一次
    /// 配置文件，子进程还在跑、工具还在注册表里，而唯一真正关停的入口（Discover Tools）在
    /// 删掉最后一个 server 之后是禁用的。
    #[test]
    fn saving_a_config_drops_servers_that_should_no_longer_be_running() {
        let server = |name: &str, enabled: bool| McpServerConfig {
            name: name.to_string(),
            command: "node".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            enabled,
            auto_approve: Vec::new(),
        };
        let config = McpConfig {
            servers: vec![server("files", true), server("shell", false)],
            ..Default::default()
        };
        let connected = vec!["files".to_string(), "shell".to_string(), "gone".to_string()];

        let dropped = servers_to_drop(&connected, &config);

        // 关掉开关的和配置里删掉的都要停；还启用着的不能停
        assert!(dropped.contains(&"shell".to_string()));
        assert!(dropped.contains(&"gone".to_string()));
        assert!(!dropped.contains(&"files".to_string()));
        assert_eq!(dropped.len(), 2);
    }

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

    #[test]
    fn short_tool_results_pass_through_unchanged() {
        assert_eq!(cap_tool_result(""), "");
        assert_eq!(cap_tool_result("ok"), "ok");
        let exact = "x".repeat(MAX_TOOL_RESULT_CHARS);
        assert_eq!(cap_tool_result(&exact), exact);
    }

    /// 参数里提到的路径要被认出来，并分清在不在工作区里。
    ///
    /// 这是 MCP 这条路上目前唯一能说出"它要动哪里"的手段：返回体是自由文本，没有写入清单。
    /// 所以这个判断必须**认得出常见形状**（嵌套对象、字符串数组），而且**不能把爬出去的
    /// 路径算成在里面** —— 那句话一旦说错，用户读到的是一条假的安心。
    #[test]
    fn argument_paths_are_split_by_the_workspace_boundary() {
        let root = std::path::Path::new(if cfg!(windows) {
            "C:\\work\\repo"
        } else {
            "/work/repo"
        });
        let arguments = r#"{
            "path": "src/app.ts",
            "edits": [{"file_path": "docs/readme.md"}, {"file_path": "../outside.txt"}],
            "files": ["a.ts", "b.ts"],
            "url": "https://example.com/not/a/path",
            "pattern": "fn \\w+"
        }"#;

        let paths = argument_paths(arguments, root);

        assert!(
            paths.inside.contains(&"src/app.ts".to_string()),
            "{:?}",
            paths
        );
        assert!(
            paths.inside.contains(&"docs/readme.md".to_string()),
            "{:?}",
            paths
        );
        assert!(paths.inside.contains(&"a.ts".to_string()), "{:?}", paths);
        assert_eq!(paths.outside, vec!["../outside.txt".to_string()]);
        // URL 和正则不是路径，混进来只会让记录变噪音
        let all = format!("{:?}", paths);
        assert!(!all.contains("example.com"), "{}", all);
        assert!(!all.contains("fn"), "{}", all);
    }

    /// 前缀像但不是同一个目录的路径算在外面，而绝对路径按边界判。
    #[test]
    fn a_sibling_directory_is_not_inside_the_workspace() {
        let (root, sibling, inside) = if cfg!(windows) {
            (
                "C:\\work\\repo",
                "C:\\work\\repo-old\\x.ts",
                "C:\\work\\repo\\src\\x.ts",
            )
        } else {
            ("/work/repo", "/work/repo-old/x.ts", "/work/repo/src/x.ts")
        };
        let arguments = format!(
            r#"{{"paths": ["{}", "{}"]}}"#,
            sibling.replace('\\', "\\\\"),
            inside.replace('\\', "\\\\")
        );

        let paths = argument_paths(&arguments, std::path::Path::new(root));

        assert_eq!(paths.outside, vec![sibling.to_string()], "{:?}", paths);
        assert_eq!(paths.inside, vec![inside.to_string()], "{:?}", paths);
    }

    /// 没提到任何路径的调用不该被描述成动过文件。
    #[test]
    fn a_call_without_paths_describes_nothing() {
        let root = std::path::Path::new(if cfg!(windows) {
            "C:\\work\\repo"
        } else {
            "/work/repo"
        });
        let paths = argument_paths(r#"{"query":"select 1","limit":10}"#, root);
        assert!(paths.is_empty());
        assert!(paths.describe().is_none());
        // 参数不是 JSON 时也一样：猜不出来就别猜
        assert!(argument_paths("not json", root).is_empty());
    }

    #[test]
    fn oversized_tool_results_are_capped_and_announced() {
        let huge = "x".repeat(MAX_TOOL_RESULT_CHARS + 500);
        let capped = cap_tool_result(&huge);

        assert!(capped.chars().count() < huge.chars().count());
        assert!(capped.starts_with(&"x".repeat(64)));
        assert!(capped.contains("500 of 64500 character(s) omitted"));
        // 头部保留：模型仍能看到返回体的开头
        assert_eq!(
            capped
                .chars()
                .take(MAX_TOOL_RESULT_CHARS)
                .collect::<String>(),
            huge.chars().take(MAX_TOOL_RESULT_CHARS).collect::<String>()
        );
    }

    #[test]
    fn multibyte_tool_results_are_capped_without_panicking() {
        let huge = "工".repeat(MAX_TOOL_RESULT_CHARS + 10);
        let capped = cap_tool_result(&huge);
        assert!(capped.starts_with("工工工"));
        assert!(capped.contains("10 of 64010 character(s) omitted"));
    }

    #[test]
    fn conflicting_qualified_names_are_dropped_not_silently_misrouted() {
        let mut claimed = HashMap::new();

        let (first, conflicts) = drop_conflicting_tools(
            vec![McpToolDescriptor {
                server: "a.b".to_string(),
                tool: "read".to_string(),
                qualified_name: qualify_tool_name("a.b", "read"),
                description: String::new(),
                input_schema: serde_json::json!({}),
                auto_approved: true,
            }],
            &mut claimed,
        );
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].qualified_name, "mcp__a_b__read");
        assert!(conflicts.is_empty());

        // 不同 server 名，sanitize 后撞成同一个限定名（`-` 会被保留，`.` 和空格都变 `_`）
        let (second, conflicts) = drop_conflicting_tools(
            vec![McpToolDescriptor {
                server: "a b".to_string(),
                tool: "read".to_string(),
                qualified_name: qualify_tool_name("a b", "read"),
                description: String::new(),
                input_schema: serde_json::json!({}),
                auto_approved: true,
            }],
            &mut claimed,
        );
        assert!(second.is_empty());
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("already claimed by server 'a.b'"));
        assert_eq!(claimed.len(), 1);
    }

    #[test]
    fn one_server_colliding_with_itself_keeps_only_the_first_tool() {
        let mut claimed = HashMap::new();
        let tools = vec!["read.file", "read_file"]
            .into_iter()
            .map(|tool| McpToolDescriptor {
                server: "files".to_string(),
                tool: tool.to_string(),
                qualified_name: qualify_tool_name("files", tool),
                description: String::new(),
                input_schema: serde_json::json!({}),
                auto_approved: false,
            })
            .collect();

        let (accepted, conflicts) = drop_conflicting_tools(tools, &mut claimed);
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].tool, "read.file");
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("tool 'read_file'"));
    }
}
