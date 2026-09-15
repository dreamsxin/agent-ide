//! 用 Chrome DevTools Protocol 的 **HTTP 端点**驱动用户已经打开的 Chrome。
//!
//! 为什么是 HTTP 而不是 WebSocket：`/json/list`、`/json/new`、`/json/activate`、
//! `/json/close` 四个 HTTP 端点就够"开一个页面、看看开了哪些页面、切过去、关掉"，
//! 而这几件事是当下真正缺的能力（打开本地预览、打开一份文档）。DOM 快照、点击、
//! 输入要走 WebSocket 上的 CDP 会话，那需要新增一个 ws 依赖，属于下一步。
//!
//! 为什么不自己拉起一个浏览器：那会变成第二套配置（profile、扩展、登录态），而用户
//! 要看的通常正是自己那个已登录的浏览器。代价是必须由用户带 `--remote-debugging-port`
//! 启动 Chrome —— 端点不存在时我们要给出这句话，而不是一句 "connection refused"。

use serde::{Deserialize, Serialize};

/// CDP 默认调试端口；`AGENT_IDE_CDP_PORT` 可以覆盖
pub const DEFAULT_CDP_PORT: u16 = 9222;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTab {
    pub id: String,
    pub title: String,
    pub url: String,
}

/// 调试端点的基地址。
///
/// 固定 `127.0.0.1`：CDP 没有任何认证，谁能连上端口就能读所有页面、拿到 cookie。
/// 允许配置成别的主机等于把这条毫无防护的通道开到网络上。
pub fn cdp_base(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

pub fn configured_port() -> u16 {
    std::env::var("AGENT_IDE_CDP_PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_CDP_PORT)
}

/// 只放行 http/https，并且拒绝把凭据写在 URL 里。
///
/// 逐条都是"点进去就已经晚了"的情况：
/// - `javascript:` 会在**当前页面**的源里执行脚本，等于拿到那个站点的会话；
/// - `file:` 让浏览器去读本地文件，绕开工作区边界；
/// - `chrome:` / `about:` 能碰浏览器自己的设置页；
/// - `data:` 是钓鱼页的经典载体，地址栏显示不出真实来源；
/// - `https://user:pass@host` 会把凭据写进历史和日志，而且是伪装域名的老手法。
pub fn normalize_target_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("A URL is required.".to_string());
    }
    // 控制字符会被浏览器悄悄吃掉，用来把 `javascript:` 藏在看起来正常的字符串里
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("URL contains control characters.".to_string());
    }

    let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
        format!(
            "Only http:// and https:// URLs are allowed, got: {}",
            trimmed
        )
    })?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Only http:// and https:// URLs are allowed, got scheme: {}",
            scheme
        ));
    }

    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("URL has no host.".to_string());
    }
    if authority.contains('@') {
        return Err(
            "URLs with embedded credentials are refused; they leak into history and are a \
             classic disguise for a different host."
                .to_string(),
        );
    }
    Ok(format!("{}://{}", scheme, rest))
}

/// 查询参数用的百分号编码。
///
/// 手写而不是引依赖：只有一个参数要编，而漏掉 `&` 或 `#` 的后果是 URL 被截断成另一个
/// 地址 —— 所以除了明确安全的字符，其余一律编码。
pub fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

pub fn new_tab_endpoint(port: u16, url: &str) -> String {
    format!("{}/json/new?{}", cdp_base(port), encode_query_value(url))
}

pub fn list_endpoint(port: u16) -> String {
    format!("{}/json/list", cdp_base(port))
}

pub fn activate_endpoint(port: u16, target_id: &str) -> String {
    format!(
        "{}/json/activate/{}",
        cdp_base(port),
        encode_query_value(target_id)
    )
}

pub fn close_endpoint(port: u16, target_id: &str) -> String {
    format!(
        "{}/json/close/{}",
        cdp_base(port),
        encode_query_value(target_id)
    )
}

/// 从 `/json/list` 的响应里挑出真正的页面。
///
/// 过滤 `type != "page"`：同一份列表里还有 service worker、扩展的背景页、iframe 目标，
/// 把它们当成"标签页"报给用户，他会在自己的浏览器里找不到对应的东西。
/// 也过滤 `devtools://`：那是 DevTools 自己的窗口。
pub fn parse_page_targets(body: &str) -> Result<Vec<BrowserTab>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("Unexpected CDP response: {}", error))?;
    let array = value
        .as_array()
        .ok_or_else(|| "CDP /json/list did not return a list.".to_string())?;

    Ok(array
        .iter()
        .filter(|target| target.get("type").and_then(|t| t.as_str()) == Some("page"))
        .filter_map(|target| {
            let url = target.get("url").and_then(|u| u.as_str()).unwrap_or("");
            if url.starts_with("devtools://") {
                return None;
            }
            Some(BrowserTab {
                id: target.get("id").and_then(|i| i.as_str())?.to_string(),
                title: target
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("(untitled)")
                    .to_string(),
                url: url.to_string(),
            })
        })
        .collect())
}

/// URL 的 origin（`scheme://host[:port]`），授权就按这个粒度给。
///
/// 按 origin 而不是按完整 URL：用户授权的是"这个站点"，页面内的路径跳转是同一次授权
/// 里的事；按完整 URL 会变成每点一下都要重新批一次，那种提示只会被无脑点掉。
pub fn origin_of(url: &str) -> Result<String, String> {
    let normalized = normalize_target_url(url)?;
    let (scheme, rest) = normalized
        .split_once("://")
        .ok_or_else(|| "URL has no scheme.".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    Ok(format!("{}://{}", scheme, authority.to_ascii_lowercase()))
}

/// 这个 origin 在不在允许清单里。
///
/// 清单为空就是**一个都不许**，而不是"没配就全放"：默认放开的清单在出事那天读起来
/// 像是用户批准过。`*` 是唯一的通配，而且必须由用户显式写进去 —— 它在界面上看得见，
/// 不是一个藏在代码里的默认。
pub fn origin_allowed(origin: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|entry| {
        let entry = entry.trim();
        entry == "*" || entry.eq_ignore_ascii_case(origin)
    })
}

/// 端点连不上时给出可执行的下一步，而不是一句网络错误。
fn unreachable_message(port: u16, error: &reqwest::Error) -> String {
    // 超时和"没人监听"要分开说：前者说明端口上有东西但不回话，让用户去查 Chrome
    // 本身（卡在弹窗、正在退出），而不是再加一遍已经加过的启动参数。
    if error.is_timeout() {
        return format!(
            "The Chrome DevTools endpoint on 127.0.0.1:{} accepted the connection but did not \
             answer within {} seconds. Check whether that port belongs to Chrome and whether \
             Chrome is blocked on a dialog.",
            port,
            REQUEST_TIMEOUT.as_secs()
        );
    }
    format!(
        "No Chrome DevTools endpoint on 127.0.0.1:{}. Start Chrome with \
         `--remote-debugging-port={}` (a normal Chrome launched without that flag cannot be \
         attached to afterwards). Underlying error: {}",
        port, port, error
    )
}

/// 每个 CDP 请求的上限。
///
/// reqwest 默认没有任何超时，而这些请求跑在 `ToolInvoker::invoke` 里 —— 那是同步的，
/// 取消标志只在两次工具调用**之间**检查。所以一个接受了连接却不回话的端点（占了
/// 9222 的别的服务、卡在 beforeunload 的 Chrome、睡眠后半开的 socket）会把整个 run
/// 永久钉住：用户点 Stop 只会把 UI 标成 Idle，后台任务还挂在那条阻塞的线程上，
/// 连外部动作记录都发不出去 —— 一个撤不回的能力，唯一的补偿就是那份记录。
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// 带超时的客户端。构造失败时退回默认客户端而不是报错：没有超时也比连不上好，
/// 而这个分支在 reqwest 里实际上不可达。
fn cdp_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

pub async fn list_tabs(port: u16) -> Result<Vec<BrowserTab>, String> {
    let response = cdp_client()
        .get(list_endpoint(port))
        .send()
        .await
        .map_err(|error| unreachable_message(port, &error))?;
    let body = response
        .text()
        .await
        .map_err(|error| format!("Read CDP response: {}", error))?;
    parse_page_targets(&body)
}

/// 新开一个标签页并把它带到前台。
///
/// Chrome 111 之后 `/json/new` 只接受 PUT（GET 会被拒），所以这里用 PUT；老版本同样
/// 接受 PUT，不需要为兼容再退回 GET。
pub async fn open_url(port: u16, raw_url: &str) -> Result<BrowserTab, String> {
    let url = normalize_target_url(raw_url)?;
    let response = cdp_client()
        .put(new_tab_endpoint(port, &url))
        .send()
        .await
        .map_err(|error| unreachable_message(port, &error))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Read CDP response: {}", error))?;
    if !status.is_success() {
        return Err(format!(
            "Chrome refused to open the tab ({}): {}",
            status, body
        ));
    }

    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("Unexpected CDP response: {} ({})", error, body))?;
    Ok(BrowserTab {
        id: value
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string(),
        title: value
            .get("title")
            .and_then(|title| title.as_str())
            .unwrap_or("(untitled)")
            .to_string(),
        url: value
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or(&url)
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每一条都是"打开之后再后悔已经来不及"的输入。
    #[test]
    fn only_http_urls_get_through() {
        assert!(normalize_target_url("https://example.com/docs").is_ok());
        assert!(normalize_target_url("http://127.0.0.1:1420/").is_ok());

        for hostile in [
            "javascript:alert(document.cookie)",
            "file:///c:/Users/admin/.codex/auth.json",
            "chrome://settings/passwords",
            "about:config",
            "data:text/html,<script>fetch('https://evil.example')</script>",
            "https://user:secret@evil.example/",
            "",
            "example.com",
        ] {
            assert!(
                normalize_target_url(hostile).is_err(),
                "should refuse: {}",
                hostile
            );
        }
    }

    /// 控制字符是把 scheme 藏起来的老手法，浏览器会悄悄把它们吃掉。
    #[test]
    fn control_characters_are_refused() {
        assert!(normalize_target_url("java\nscript:alert(1)").is_err());
        assert!(normalize_target_url("https://example.com/\u{0}").is_err());
    }

    /// 漏编码一个 `&` 或 `#`，URL 就被截断成另一个地址。
    #[test]
    fn query_encoding_covers_the_separators() {
        let encoded = encode_query_value("https://example.com/a?b=1&c=2#frag");
        assert!(!encoded.contains('&'));
        assert!(!encoded.contains('#'));
        assert!(!encoded.contains('?'));
        assert!(encoded.contains("https%3A%2F%2Fexample.com"));
    }

    /// 只报真正的页面：service worker 和扩展背景页在用户的浏览器里找不到对应的东西。
    #[test]
    fn only_pages_are_reported_as_tabs() {
        let body = r#"[
            {"id":"1","type":"page","title":"Docs","url":"https://example.com/docs"},
            {"id":"2","type":"service_worker","title":"sw","url":"https://example.com/sw.js"},
            {"id":"3","type":"page","title":"DevTools","url":"devtools://devtools/bundled/x.html"},
            {"id":"4","type":"background_page","title":"ext","url":"chrome-extension://abc/bg.html"}
        ]"#;

        let tabs = parse_page_targets(body).unwrap();

        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].id, "1");
        assert_eq!(tabs[0].url, "https://example.com/docs");
    }

    #[test]
    fn a_non_list_response_is_an_error_not_an_empty_list() {
        assert!(parse_page_targets("{\"error\":\"nope\"}").is_err());
        assert!(parse_page_targets("not json").is_err());
    }

    /// 端点固定在回环地址：CDP 没有认证，连上就能读所有页面和 cookie。
    #[test]
    fn endpoints_stay_on_loopback() {
        assert_eq!(cdp_base(9222), "http://127.0.0.1:9222");
        assert!(list_endpoint(1234).starts_with("http://127.0.0.1:1234/json/list"));
        assert!(activate_endpoint(9222, "AB/CD").contains("AB%2FCD"));
    }

    #[test]
    fn origin_is_scheme_host_and_port() {
        assert_eq!(
            origin_of("https://Example.COM/docs/a?b=1").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            origin_of("http://127.0.0.1:1420/index.html").unwrap(),
            "http://127.0.0.1:1420"
        );
        // 端口不同就是另一个 origin：dev server 和线上站点不该共用一次授权
        assert_ne!(
            origin_of("http://127.0.0.1:1420/").unwrap(),
            origin_of("http://127.0.0.1:4173/").unwrap()
        );
    }

    /// 空清单是"一个都不许"。默认放开的清单在出事那天读起来像是用户批准过。
    #[test]
    fn an_empty_allowlist_allows_nothing() {
        assert!(!origin_allowed("https://example.com", &[]));
        assert!(origin_allowed(
            "https://example.com",
            &["https://example.com".to_string()]
        ));
        assert!(!origin_allowed(
            "https://evil.example",
            &["https://example.com".to_string()]
        ));
        // 大小写不敏感，但子域名不算：`a.example.com` 不在 `example.com` 的授权里
        assert!(origin_allowed(
            "https://example.com",
            &["https://EXAMPLE.com".to_string()]
        ));
        assert!(!origin_allowed(
            "https://a.example.com",
            &["https://example.com".to_string()]
        ));
        assert!(origin_allowed(
            "https://anything.example",
            &["*".to_string()]
        ));
    }
}
