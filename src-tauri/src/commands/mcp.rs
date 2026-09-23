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
///
/// 取消开关从**这次运行的授权**里取，不接受单独传进来的 `Arc`：MCP 是本产品最大的副作用
/// 面，而"调用方记得传对那一个开关"是靠不住的约定 —— 传错编译器不会说话，后果是 Stop
/// 之后 MCP 调用照旧一个个发出去。内置工具面和 `try_begin_run` 走的是同一条规则。
pub async fn attach_mcp_tools(
    registry: &Arc<McpRegistry>,
    events: Arc<dyn RunEvents>,
    llm: LlmClient,
    policy: McpToolPolicy,
    permissions: &crate::agent::workspace_tools::WorkspaceToolPermissions,
) -> (LlmClient, Option<Arc<dyn ToolInvoker>>) {
    let definitions = registry.tool_definitions(policy).await;
    if definitions.is_empty() {
        return (llm, None);
    }
    let invoker: Arc<dyn ToolInvoker> = Arc::new(McpToolInvoker {
        registry: registry.clone(),
        events,
        policy,
        permissions: permissions.clone(),
    });
    (llm.with_extra_tools(definitions), Some(invoker))
}

/// 一次调用前后各读一遍的文件，单个最多这么大。
///
/// 上限存在的理由是延迟而不是内存：参数里提到 lockfile 那种尺寸的文件时，读两遍会让每次
/// MCP 调用都明显变慢。超过的如实记成"没覆盖"，不假装能撤销。
const MAX_PROBE_BYTES: u64 = 1024 * 1024;

struct McpToolInvoker {
    registry: Arc<McpRegistry>,
    /// 发事件走 trait，不直接持 `AppHandle`。
    ///
    /// 持 `AppHandle` 的版本在单元测试里根本没法构造，于是"Stop 之后拒绝调用"这条
    /// 分支一行都没有覆盖 —— 和 orchestrator 当初的问题是同一个。
    events: Arc<dyn RunEvents>,
    policy: McpToolPolicy,
    /// 这次运行的授权。取消开关和外部动作记录都从这里取，不各存一份。
    ///
    /// MCP 是本产品最大的副作用面（文件系统、git、HTTP 服务器都可能挂在这里），而它做过
    /// 什么，此前**一条持久记录都没有**：调用只进内存里的 action log，关窗即失。所以这里
    /// 要的不只是取消开关，而是整份授权 —— 一次 MCP 调用撤不回来，撤不回来的动作就该走
    /// 那条"事后查得到"的通道，和导航、截图、`web_fetch` 同一条。
    permissions: crate::agent::workspace_tools::WorkspaceToolPermissions,
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
        self.events.emit_json(
            "agent-action-log",
            serde_json::to_value(entry).unwrap_or_default(),
        );
    }

    /// 把这次调用记进持久的外部动作日志。
    ///
    /// 记的是"调了什么、它说要动哪里"，不是"改了哪些文件"—— 后者我们现在还看不见（MCP 的
    /// 返回体是自由文本，没有写入清单）。把看得见的那部分老实记下来，比什么都不记好；也比
    /// 假装记全了好。
    fn record(&self, kind: &str, tool_name: &str, detail: String) {
        self.permissions
            .record_external(crate::agent::workspace_tools::AgentExternalAction {
                kind: kind.to_string(),
                target: tool_name.to_string(),
                detail,
            });
    }

    /// 参数里提到的路径，按工作区边界分开。工作区根取不到时返回 `None`。
    fn paths_named(&self, arguments: &str) -> Option<crate::services::mcp::ArgumentPaths> {
        let root = crate::services::workspace::workspace_root().ok()?;
        let paths = crate::services::mcp::argument_paths(arguments, &root);
        if paths.is_empty() {
            None
        } else {
            Some(paths)
        }
    }

    /// 调用**之前**给参数里提到的工作区内文件各留一份底。
    ///
    /// 这是让 MCP 写入变得可撤销的唯一办法：MCP 的返回体是自由文本，没有写入清单，工具
    /// 也不会告诉我们它改了什么。所以只能自己在调用两侧各看一眼 —— 这正是内置工具做的事
    /// （先读旧内容再写），只不过这里的"旧内容"要由我们代它保存。
    ///
    /// 参考实现的 checkpoint 要求工具在结构化输出里自报 `originalFile`，而 MCP 的输出
    /// schema 装不下那个字段，于是它对 MCP 一律不做 checkpoint。这里不照搬那个结论：
    /// 自报不了就替它记，代价是一次调用多读几个文件。
    fn snapshot_targets(&self, arguments: &str) -> (Vec<FileProbe>, usize) {
        let Ok(root) = crate::services::workspace::workspace_root() else {
            return (Vec::new(), 0);
        };
        let mut probes = Vec::new();
        let mut uncovered = 0usize;
        for path in crate::services::mcp::argument_targets(arguments, &root) {
            let metadata = std::fs::metadata(&path);
            let existed = metadata
                .as_ref()
                .map(|meta| meta.is_file())
                .unwrap_or(false);
            // 太大的文件不留底：一次调用可能提到好几个，而 lockfile 那种尺寸的内容读两遍
            // 会把每次 MCP 调用都变慢。宁可如实说"这个没覆盖"
            if existed && metadata.map(|meta| meta.len()).unwrap_or(0) > MAX_PROBE_BYTES {
                uncovered += 1;
                continue;
            }
            let before = std::fs::read_to_string(&path).ok();
            // 存在但读不出来（二进制、非 UTF-8）：没有能写回去的底，不假装能撤销
            if existed && before.is_none() {
                uncovered += 1;
                continue;
            }
            let file = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            probes.push(FileProbe {
                file,
                path,
                before,
                existed,
            });
        }
        (probes, uncovered)
    }

    /// 调用**之后**再看一眼，把真的变了的文件记成可撤销的写入。返回记下了几个。
    fn publish_detected_changes(&self, tool_name: &str, probes: Vec<FileProbe>) -> usize {
        let mut changed = 0usize;
        for probe in probes {
            let exists_now = probe.path.is_file();
            let after = std::fs::read_to_string(&probe.path).ok();
            if let Some(write) = write_from_probe(&probe, exists_now, after) {
                self.permissions
                    .record_detected_write(tool_name.to_string(), write);
                changed += 1;
            }
        }
        changed
    }
}

/// 比对前后两次读到的东西，决定这是不是一次要记下来的写入。
///
/// 纯函数：判断本身就是这个功能的全部内容 —— 什么算"变了"、删除长什么样、读不出来时认不认。
/// 夹在两次 `fs::read` 中间的话，这四条分支只能靠真跑一个 MCP server 才验证得到。
fn write_from_probe(
    probe: &FileProbe,
    exists_now: bool,
    after: Option<String>,
) -> Option<crate::agent::workspace_tools::AgentFileWrite> {
    let build = |previous: Option<String>, updated: String, removed: bool| {
        Some(crate::agent::workspace_tools::AgentFileWrite {
            file: probe.file.clone(),
            path: probe.path.clone(),
            previous,
            updated,
            removed,
            moved_from: None,
        })
    };

    if probe.existed && !exists_now {
        // 删除：写前内容还在手里，撤销就是把文件写回去
        return build(probe.before.clone(), String::new(), true);
    }
    // 现在读不出来（被换成二进制了？）：说不出内容，不猜
    let now = after?;
    if probe.before.as_deref() == Some(now.as_str()) {
        return None;
    }
    // 调用前不存在的就是新建（`previous: None`），撤销时删掉它
    build(probe.before.clone(), now, false)
}

/// 一次 MCP 调用之前给一个文件留下的底。
struct FileProbe {
    /// 工作区相对路径，给审查卡片用
    file: String,
    path: std::path::PathBuf,
    /// 调用前的内容。`None` 表示调用前这个文件不存在
    before: Option<String>,
    existed: bool,
}

#[async_trait]
impl ToolInvoker for McpToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        is_mcp_tool_name(tool_name)
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        // Stop 之后不再往外发调用。MCP 工具做什么我们一概不知道，所以这里只能做能做的
        // 那件事：不开始新的。已经在飞的那一次拦不住 —— 那需要 MCP 客户端支持取消。
        if self.permissions.cancelled() {
            let detail = format!(
                "This run was stopped, so MCP tool {} was not called.",
                tool_name
            );
            self.log(
                "warn",
                &format!("Refused {} after Stop", tool_name),
                &detail,
            );
            self.record("mcp_tool_cancelled", tool_name, detail.clone());
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
        let named = self.paths_named(arguments);
        let (probes, uncovered) = self.snapshot_targets(arguments);
        match self.registry.call(tool_name, arguments, self.policy).await {
            Ok(result) => {
                self.log(
                    "success",
                    &format!("MCP tool {} returned {} chars", tool_name, result.len()),
                    &truncate(&result, 2000),
                );
                let changed = self.publish_detected_changes(tool_name, probes);
                let mut detail = format!(
                    "Called MCP tool {}; it returned {} character(s).",
                    tool_name,
                    result.chars().count()
                );
                if let Some(sentence) = named.as_ref().and_then(|paths| paths.describe()) {
                    detail.push(' ');
                    detail.push_str(&sentence);
                    detail.push('.');
                }
                if changed > 0 {
                    detail.push_str(&format!(
                        " {} file(s) changed; they are in the review area as applied diffs and Undo Apply restores them.",
                        changed
                    ));
                }
                let outside = named.as_ref().map(|paths| paths.outside.len()).unwrap_or(0);
                if uncovered > 0 || outside > 0 {
                    // 覆盖不到的那部分要自己说出来：读的人看到"1 file(s) changed"会以为那就是全部
                    detail.push_str(&format!(
                        " Not covered: {} path(s) outside the workspace and {} file(s) too large or not text — changes there are neither shown nor undoable.",
                        outside, uncovered
                    ));
                }
                self.record("mcp_tool_call", tool_name, detail);
                Ok(result)
            }
            Err(error) => {
                self.log("error", &format!("MCP tool {} failed", tool_name), &error);
                // 失败也要比对：一次报错的调用照样可能已经把文件写了一半 —— 错误是 server
                // 说的，不是它没动手的证明
                let changed = self.publish_detected_changes(tool_name, probes);
                let mut detail = format!("MCP tool {} failed: {}", tool_name, error);
                if changed > 0 {
                    detail.push_str(&format!(
                        " It had already changed {} file(s); they are in the review area and Undo Apply restores them.",
                        changed
                    ));
                }
                self.record("mcp_tool_failed", tool_name, detail);
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

/// 保存配置，并把这次保存之后不该再活着的 server 停掉。
///
/// 停掉那一步是必须的，不是顺手做的：面板里关掉或者删掉一个 server 只会 `persist` 一次配置，
/// 而真正的关停原来只发生在 `discover` 里。也就是说"关掉"之后子进程还在跑、工具还在注册表
/// 里；删掉最后一个 server 之后 Discover Tools 按钮还是禁用的，那时除了重启应用没有别的办法。
#[tauri::command]
pub async fn save_mcp_config(
    config: McpConfig,
    mcp_state: State<'_, McpState>,
) -> Result<McpConfig, String> {
    save_config(&config)?;
    mcp_state.registry.retain_configured(&config).await;
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

#[cfg(test)]
mod tests {
    use super::{redact_arguments, truncate, write_from_probe, FileProbe, McpToolInvoker};
    use crate::agent::events::RecordingEvents;
    use crate::agent::executor::ToolInvoker;
    use crate::agent::workspace_tools::WorkspaceToolPermissions;
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
        let mut permissions = WorkspaceToolPermissions::read_only();
        permissions.adopt_cancel(cancel.clone());
        let invoker = McpToolInvoker {
            registry: Arc::new(McpRegistry::new()),
            events: events.clone(),
            policy: McpToolPolicy::AllowAll,
            permissions: permissions.clone(),
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
        // 而且要进那份活过关窗的记录
        let actions = permissions.take_external_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "mcp_tool_cancelled");
    }

    /// 没被取消时闸门不挡路：拒绝要来自策略或注册表，不能来自开关。
    #[test]
    fn an_active_run_is_not_blocked_by_the_switch() {
        let events = Arc::new(RecordingEvents::new());
        let mut permissions = WorkspaceToolPermissions::read_only();
        permissions.adopt_cancel(Arc::new(AtomicBool::new(false)));
        let invoker = McpToolInvoker {
            registry: Arc::new(McpRegistry::new()),
            events: events.clone(),
            policy: McpToolPolicy::AllowAll,
            permissions: permissions.clone(),
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
        // 失败的调用也进持久记录：一句报错不是"它没动手"的证明
        let actions = permissions.take_external_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "mcp_tool_failed");
    }

    /// 一个被改过的文件要变成"有写前内容、能撤销"的一条写入。
    ///
    /// 这是 MCP 写入从"看不见"变成"可撤销"的全部机制：调用前留一份底，调用后比一比。
    /// 四条分支各有一种错法 —— 把没变的记成改过（审查区出现假卡片）、把删除记成清空
    /// （用户以为文件还在）、把新建记成编辑（撤销时写回一个空字符串而不是删掉）、
    /// 读不出来时瞎猜内容（撤销会把文件毁掉）。
    #[test]
    fn a_probe_turns_a_real_change_into_an_undoable_write() {
        let probe = |before: Option<&str>| FileProbe {
            file: "src/app.ts".to_string(),
            path: std::path::PathBuf::from("/w/src/app.ts"),
            before: before.map(|text| text.to_string()),
            existed: before.is_some(),
        };

        // 改过：写前内容进 previous，撤销就是写回去
        let edited = write_from_probe(&probe(Some("old")), true, Some("new".to_string()))
            .expect("changed file");
        assert_eq!(edited.previous.as_deref(), Some("old"));
        assert_eq!(edited.updated, "new");
        assert!(!edited.removed);

        // 没变：不能进审查区，否则那里全是假卡片
        assert!(write_from_probe(&probe(Some("same")), true, Some("same".to_string())).is_none());

        // 新建：previous 是 None，撤销时删掉它而不是写一个空文件
        let created =
            write_from_probe(&probe(None), true, Some("fresh".to_string())).expect("created file");
        assert!(created.previous.is_none());
        assert_eq!(created.updated, "fresh");

        // 删除：标成 removed，卡片上才不会显示成"文件被清空"
        let deleted =
            write_from_probe(&probe(Some("gone soon")), false, None).expect("deleted file");
        assert!(deleted.removed);
        assert_eq!(deleted.previous.as_deref(), Some("gone soon"));

        // 现在读不出来（变成二进制）：说不出内容就不记，撤销不能拿猜出来的内容覆盖文件
        assert!(write_from_probe(&probe(Some("text")), true, None).is_none());
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
