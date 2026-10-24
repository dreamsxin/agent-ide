//! 用 Chrome DevTools Protocol 驱动用户已经打开的 Chrome。
//!
//! 两条通道，各干一件事：
//! - **HTTP**（`/json/list`、`/json/new`）足够"开一个页面、看看开了哪些页面"。
//! - **WebSocket**（`Runtime.evaluate`）是读页面内容唯一的通道 —— HTTP 端点给不出
//!   正文，而"这一页上写了什么"恰好是开了页面之后最常缺的一步。
//!
//! 为什么不自己拉起一个浏览器：那会变成第二套配置（profile、扩展、登录态），而用户
//! 要看的通常正是自己那个已登录的浏览器。代价是必须由用户带 `--remote-debugging-port`
//! 启动 Chrome —— 端点不存在时我们要给出这句话，而不是一句 "connection refused"。

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

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

/// 从 `/json/list` 的响应里挑出真正的页面。
///
/// 过滤 `type != "page"`：同一份列表里还有 service worker、扩展的背景页、iframe 目标，
/// 把它们当成"标签页"报给用户，他会在自己的浏览器里找不到对应的东西。
/// 也过滤 `devtools://`：那是 DevTools 自己的窗口。
pub fn parse_page_targets(body: &str) -> Result<Vec<BrowserTab>, String> {
    Ok(parse_page_sessions(body)?
        .into_iter()
        .map(|session| session.tab)
        .collect())
}

/// 一个可以被附着的页面：列表里的那一条 + 它的调试 WebSocket。
///
/// 和 `BrowserTab` 分开而不是给它加一个字段：`BrowserTab` 会经 `browser_list_tabs`
/// 原样序列化给前端，而调试 socket 地址是一条能在那个页面的源里执行任意脚本的通道 ——
/// 它只该出现在真的要用它的那条路径上。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSession {
    pub tab: BrowserTab,
    /// `webSocketDebuggerUrl`。已经开着 DevTools 的页面 Chrome 不给这个字段。
    ///
    /// 记成 `None` 而不是把这条页面过滤掉：过滤掉之后"没有这个页面"和"这个页面正开着
    /// DevTools 所以读不了"在报错里长得一模一样，而这两句话给用户的下一步完全不同。
    pub ws_url: Option<String>,
}

/// 解析成带调试通道的页面列表。`parse_page_targets` 是它丢掉 socket 之后的视图。
pub fn parse_page_sessions(body: &str) -> Result<Vec<PageSession>, String> {
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
            Some(PageSession {
                tab: BrowserTab {
                    id: target.get("id").and_then(|i| i.as_str())?.to_string(),
                    title: target
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("(untitled)")
                        .to_string(),
                    url: url.to_string(),
                },
                ws_url: target
                    .get("webSocketDebuggerUrl")
                    .and_then(|u| u.as_str())
                    .filter(|u| !u.is_empty())
                    .map(str::to_string),
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
///
/// **不走代理**。reqwest 默认会读 `HTTP_PROXY` / `ALL_PROXY`，于是在设了代理的机器上
/// （公司网络里很常见），发往 127.0.0.1 的 CDP 请求会被送去代理。两个后果都不能接受：
/// 浏览器工具会以一堆看不懂的错误静默失效，而不是给出那句"用 --remote-debugging-port
/// 启动 Chrome"；更糟的是 `open_url` 把目标 URL 放在请求行里，经代理就等于把"Agent 正在
/// 打开什么"泄露给了一个用户从没授权过的第三方。这条是一次测试意外发现的：连一个没人监听
/// 的端口居然返回了空响应体，而不是连接被拒。
fn cdp_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(REQUEST_TIMEOUT)
        .no_proxy()
        .build()
        .unwrap_or_default()
}

/// 取一次 `/json/list` 的原始响应。
///
/// 抽出来是因为它有两个读者（标签页列表、要读的那一页），而"忘了给新读者加超时"或者
/// "忘了给它那句可执行的连不上提示"都是这里出过的那类缺陷。
async fn fetch_list_body(port: u16) -> Result<String, String> {
    let response = cdp_client()
        .get(list_endpoint(port))
        .send()
        .await
        .map_err(|error| unreachable_message(port, &error))?;
    response
        .text()
        .await
        .map_err(|error| format!("Read CDP response: {}", error))
}

pub async fn list_tabs(port: u16) -> Result<Vec<BrowserTab>, String> {
    parse_page_targets(&fetch_list_body(port).await?)
}

/// 和 `list_tabs` 同一份响应，但保留每个页面的调试 socket —— 读正文要用它。
pub async fn list_page_sessions(port: u16) -> Result<Vec<PageSession>, String> {
    parse_page_sessions(&fetch_list_body(port).await?)
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

/// 页面正文最多回传给模型的字符数。
///
/// 比 `workspace_read_file` 的 64 000 小得多：一个文档站点的 `innerText` 里除了正文还有
/// 导航、页脚、Cookie 横幅，放开上限只会让一次调用吃掉整个上下文预算。截断发生在
/// **页面里**（见 `page_text_expression`），所以这个数同时是"取多少"和"传多少"。
pub const MAX_PAGE_TEXT_CHARS: usize = 20_000;

/// 一次读到的页面正文。
///
/// `chars` 是截断**之前**的长度：模型只有知道"还有多少没看到"才判断得出该不该换个
/// 更窄的读法，只给一段掐断的文本会让它以为自己读完了。
///
/// `url` 是**页面自己报的**当时的地址，不是 `/json/list` 里那个 —— 它和正文来自同一次
/// 求值，所以它是唯一能用来复核"读到的确实是批准过的那个站点"的东西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageText {
    pub url: String,
    pub text: String,
    pub truncated: bool,
    pub chars: usize,
}

/// 选定要读的那一页：`PageSession` 减掉"可能没有调试通道"这个可能性。
///
/// 单独一个类型而不是在调用点 `unwrap` 那个 `Option`：读正文必须有 socket，把这条
/// 前提交给类型之后，调用点里那个"理论上不会发生"的分支就不存在了 —— 而那种分支是
/// 后来真的发生过的那类。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadTarget {
    pub tab: BrowserTab,
    pub ws_url: String,
}

/// 在已经打开的页面里选出**唯一**一个要读的。
///
/// 至少要给一个筛选条件。空条件会命中每一个被允许的页面，而"命中多个"的拒绝话术里带着
/// 候选页面的标题和 URL —— 于是一次不带参数的调用就成了 `workspace_browser_tabs`，绕开
/// 了那个工具自己的授权。截图那边是同一条规则（`select_capture_target` 也拒绝空条件）。
/// 代价是模型必须先知道一个子串；这正是"读哪一页"本来就该带的信息。
///
/// 清单过滤的是**候选**，不只是决定工具存不存在：不在允许 origin 里的页面既不参与匹配，
/// 也不出现在候选列表里 —— 否则允许清单只限制"能读到什么"，却不限制"能看见有什么"。
///
/// 命中多个就拒绝而不是挑第一个：读错一页的代价是把另一个站点的内容交给模型，而"第一个"
/// 取决于 Chrome 的返回顺序，没有任何人能预期它。
pub fn select_read_target(
    sessions: Vec<PageSession>,
    url_contains: Option<&str>,
    title_contains: Option<&str>,
    allowlist: &[String],
) -> Result<ReadTarget, String> {
    if url_contains.is_none() && title_contains.is_none() {
        return Err(
            "Name the page with 'url_contains' and/or 'title_contains' — reading is per page, \
             so an empty filter is refused rather than resolved to whatever is open."
                .to_string(),
        );
    }
    let total = sessions.len();
    let allowed: Vec<PageSession> = sessions
        .into_iter()
        .filter(|session| {
            origin_of(&session.tab.url)
                .map(|origin| origin_allowed(&origin, allowlist))
                .unwrap_or(false)
        })
        .collect();
    let excluded = total - allowed.len();
    let mut matched: Vec<PageSession> = allowed
        .into_iter()
        .filter(|session| {
            contains_ignoring_case(&session.tab.url, url_contains)
                && contains_ignoring_case(&session.tab.title, title_contains)
        })
        .collect();

    if matched.is_empty() {
        return Err(format!(
            "No readable page matches {}.{}",
            describe_read_filter(url_contains, title_contains),
            outside_note(excluded)
        ));
    }
    if matched.len() > 1 {
        let candidates = matched
            .iter()
            .map(|session| format!("- {} — {}", session.tab.title, session.tab.url))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "{} readable pages match {}, so nothing was read. Narrow it down:\n{}",
            matched.len(),
            describe_read_filter(url_contains, title_contains),
            candidates
        ));
    }

    let session = matched.remove(0);
    match session.ws_url {
        Some(ws_url) => Ok(ReadTarget {
            tab: session.tab,
            ws_url,
        }),
        None => Err(format!(
            "\"{}\" cannot be attached to — Chrome only offers one debugger per page, so close \
             its DevTools window and try again.",
            session.tab.title
        )),
    }
}

/// 被清单挡在候选之外的页面数量。
///
/// 只说数量，不说是哪些：这句话是给一次**被拒**的调用看的，而模型问的那个筛选条件本来
/// 就不该顺带换来一份"你还有这些页面开着"的清单。
fn outside_note(excluded: usize) -> String {
    if excluded == 0 {
        return String::new();
    }
    format!(
        " {} other open page(s) are outside this run's allowed origins and were not searched.",
        excluded
    )
}

fn contains_ignoring_case(haystack: &str, needle: Option<&str>) -> bool {
    match needle {
        Some(needle) => haystack.to_lowercase().contains(&needle.to_lowercase()),
        None => true,
    }
}

/// 把筛选条件写成一句人话。
///
/// 拼出来而不是穷举四种组合：`select_read_target` 已经拒掉了"两个都没给"，穷举就得留一个
/// 到不了的分支 —— 而到不了的分支后来总会有人走到。
fn describe_read_filter(url_contains: Option<&str>, title_contains: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(url) = url_contains {
        parts.push(format!("a URL containing \"{}\"", url));
    }
    if let Some(title) = title_contains {
        parts.push(format!("a title containing \"{}\"", title));
    }
    parts.join(" and ")
}

/// 取正文的固定表达式。
///
/// 这个函数只收一个字符数，**不收**表达式：`Runtime.evaluate` 在那个页面的源里执行，
/// 一个模型能决定内容的表达式等于把这个站点的会话整个交出去（读 cookie、以用户身份
/// 发请求）。用户批准的是"读这一页写了什么"，那就只该有一段写死的取文本脚本。
///
/// 表达式**连同 `location.href` 一起返回**，这是这段脚本最重要的一件事：授权是按 origin
/// 给的，而那个判断此前用的是 `/json/list` 快照里的 URL —— 调试 socket 绑的是 target，
/// 页面在"列出"和"读到"之间导航到别处时，socket 照样有效，于是读到的是一个没人授权过的
/// 站点。URL 和正文来自**同一次求值**，所以这个复核没有缝可钻。
///
/// 截断在页面里做：一份长文档的 `innerText` 可以是几 MB，这些字节没人要看，却要先过
/// 一遍 WebSocket 帧再在这边丢掉。按**码位**切（`Array.from`）而不是 `slice`：JS 的
/// `slice` 数的是 UTF-16 单元，正好切在一个星文平面字符中间会留下半个代理对，那不是
/// 合法 JSON，整次读取会以一句"响应不对"结束。
pub fn page_text_expression(max_chars: usize) -> String {
    format!(
        "(() => {{ const raw = (document.body && document.body.innerText) || ''; \
         const points = Array.from(raw.replace(/\\n{{3,}}/g, '\\n\\n').trim()); \
         return {{ url: location.href, text: points.slice(0, {max}).join(''), \
         chars: points.length, truncated: points.length > {max} }}; }})()",
        max = max_chars
    )
}

/// 只连回环上属于这个端口的调试 socket。
///
/// 地址来自 Chrome 自己的响应，正常情况下就是 `ws://127.0.0.1:<port>/devtools/page/<id>`。
/// 仍然要查一遍：这是一个由**响应内容**决定我们去连哪里的字段，端口被别的服务占着、
/// 或者响应经过了什么东西转发时，我们会把脚本送到一个陌生的地方去执行。
///
/// 结尾那个 `/` 是这道检查的全部力气所在，不是格式上的讲究：少了它，
/// `ws://127.0.0.1:9222@evil.example/x` 就能通过 —— userinfo 的 `@` 恰好占住 authority
/// 的位置，而真正被连接的主机是 `evil.example`。
pub fn validate_page_ws_url(ws_url: &str, port: u16) -> Result<(), String> {
    let expected = format!("ws://127.0.0.1:{}/", port);
    if ws_url.starts_with(&expected) {
        return Ok(());
    }
    Err(format!(
        "Refusing to attach to {}: the page debugger must be on {}",
        ws_url, expected
    ))
}

/// 读回来的那一页，是不是用户批准的那一页。
///
/// 比的是 origin 而不是完整 URL：授权本来就是按 origin 给的，同一个站点内的跳转属于
/// 同一次授权（和 `browser_open` 一致）。跨 origin 就拒绝，而且**不说**跳到哪儿去了 ——
/// 那正是一个没被授权的站点，把它的地址写进返回值等于替这次被拒的读取完成了披露。
///
/// 这是 `ApprovedWindow::verify` 在浏览器侧的对应物：那边靠句柄 + pid 认窗口，这边靠
/// target 的 socket 认页面（socket 就绑在 target 上，所以身份是结构保证的），需要复核的
/// 只剩"它现在在哪个站点"。
pub fn verify_read_origin(approved_origin: &str, read_url: &str) -> Result<(), String> {
    match origin_of(read_url) {
        Ok(origin) if origin == approved_origin => Ok(()),
        _ => Err(format!(
            "That page navigated away from {} after it was approved, so nothing was read.",
            approved_origin
        )),
    }
}

/// 解析 `Runtime.evaluate` 的回答。
///
/// 三种失败都要分开说：CDP 层拒绝（`error`）、脚本在页面里抛了（`exceptionDetails`）、
/// 回答里没有我们要的形状。把它们都变成"读到了空文本"会让模型以为页面是空的，然后
/// 据此得出结论 —— 一个静默的错误在这条链上比一次失败贵得多。
pub fn parse_evaluate_response(body: &str) -> Result<PageText, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("Unexpected CDP response: {}", error))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("unknown error");
        return Err(format!("Chrome refused to read that page: {}", message));
    }
    let result = value
        .get("result")
        .ok_or_else(|| "CDP answered without a result.".to_string())?;
    if let Some(exception) = result.get("exceptionDetails") {
        let text = exception
            .get("text")
            .and_then(|text| text.as_str())
            .unwrap_or("the page's own script threw");
        return Err(format!(
            "Reading that page failed inside the page: {}",
            text
        ));
    }
    let payload = result
        .get("result")
        .and_then(|inner| inner.get("value"))
        .ok_or_else(|| "CDP returned no value for the page text.".to_string())?;
    let text = payload
        .get("text")
        .and_then(|text| text.as_str())
        .ok_or_else(|| "CDP returned a value without page text.".to_string())?;
    // URL 缺了就是错误，不是"那就不查了"：这个字段是跨 origin 复核的唯一依据，
    // 悄悄退回"没有 URL 也放行"会把整道复核变成装饰。
    let url = payload
        .get("url")
        .and_then(|url| url.as_str())
        .ok_or_else(|| "CDP returned page text without the page's own URL.".to_string())?;
    Ok(PageText {
        url: url.to_string(),
        text: text.to_string(),
        truncated: payload
            .get("truncated")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(false),
        chars: payload
            .get("chars")
            .and_then(|chars| chars.as_u64())
            .unwrap_or(text.chars().count() as u64) as usize,
    })
}

/// 读一个页面的可见文本。
///
/// 整个交换套一层超时：`REQUEST_TIMEOUT` 对 HTTP 请求是 reqwest 在管，而 WebSocket 上
/// 没有任何默认超时 —— 一个连上了却不回答的页面（卡在 `beforeunload`、主线程被自己的
/// 脚本占死）会把这次工具调用永久钉住，而取消只在两次调用之间生效。
pub async fn read_page_text(ws_url: &str, port: u16, max_chars: usize) -> Result<PageText, String> {
    validate_page_ws_url(ws_url, port)?;
    match tokio::time::timeout(REQUEST_TIMEOUT, evaluate_page_text(ws_url, max_chars)).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "That page accepted the DevTools connection but did not answer within {} seconds; \
             its main thread may be busy.",
            REQUEST_TIMEOUT.as_secs()
        )),
    }
}

/// CDP 请求的 id。一条 socket 上只发一个请求，所以固定值够用 —— 但回答仍然要按 id 认。
const EVALUATE_REQUEST_ID: i64 = 1;

async fn evaluate_page_text(ws_url: &str, max_chars: usize) -> Result<PageText, String> {
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|error| {
            format!(
                "Could not attach to that page over the DevTools protocol: {}",
                error
            )
        })?;
    let request = serde_json::json!({
        "id": EVALUATE_REQUEST_ID,
        "method": "Runtime.evaluate",
        "params": {
            "expression": page_text_expression(max_chars),
            "returnByValue": true,
            // 不等 promise：一个 `await` 得到的值要由页面自己的代码决定何时兑现，而这段
            // 脚本是同步的。开着它只会把"页面卡住"变成"我们卡住"。
            "awaitPromise": false
        }
    });
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .map_err(|error| format!("Could not send the read request: {}", error))?;

    let answer = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                // 按 id 认回答：这条 socket 上也会来 CDP 事件，把第一条消息当结果会让
                // 一个无关的事件被解析成"这一页是空的"。
                let parsed: serde_json::Value =
                    serde_json::from_str(text.as_str()).unwrap_or(serde_json::Value::Null);
                if parsed.get("id").and_then(|id| id.as_i64()) == Some(EVALUATE_REQUEST_ID) {
                    break text.as_str().to_string();
                }
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err("Chrome closed the DevTools connection before answering.".to_string())
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(format!("The DevTools connection failed: {}", error)),
        }
    };
    // 不 `await` 关闭握手：这整段都在超时里，而一个不肯把 close 帧冲出去的对端会让一次
    // **已经成功**的读取以"页面没有在 10 秒内回答"结束 —— 一句假话。丢掉 socket 就关掉了
    // 底层连接，剩下的礼貌由 Chrome 自己回收。
    drop(socket);
    parse_evaluate_response(&answer)
}

/// 假的 CDP HTTP 端点，供测试使用。
///
/// 放在生产模块里（`#[cfg(test)]`）而不是各个测试模块里各写一份：`workspace_tools` 那边
/// 也需要一个，而两份假服务迟早会在"哪个端点回什么"上分叉 —— 那时两边测的就不是同一个
/// 协议了。
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, Mutex};

    pub struct FakeCdp {
        pub port: u16,
        /// 收到的请求行，按顺序。断言"用的是 PUT /json/new"要靠它。
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    impl FakeCdp {
        pub fn request_lines(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    /// 起一个只服务 `connections` 个连接的假 CDP。
    ///
    /// 按请求行里有没有 `/json/new` 决定回哪份 body。故意不做完整路由：这些测试要验的是
    /// 我们的客户端**发了什么**、以及**怎么解析回来的东西**，不是重写一个 Chrome。
    pub async fn spawn(list_body: String, new_body: String, connections: usize) -> FakeCdp {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0_u8; 2048];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let first_line = request.lines().next().unwrap_or_default().to_string();
                let body = if first_line.contains("/json/new") {
                    new_body.clone()
                } else {
                    list_body.clone()
                };
                recorded
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(first_line);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                     {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        FakeCdp { port, requests }
    }
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
        assert!(new_tab_endpoint(9222, "https://example.com/a b")
            .starts_with("http://127.0.0.1:9222/json/new?"));
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

    /// 连得上的时候，标签页列表真的从 HTTP 上读回来。
    ///
    /// `list_tabs` 和 `open_url` 此前一行覆盖都没有 —— 所有测试都停在解析函数上，而
    /// "请求发去了哪个端点、用的什么方法"恰好是 CDP 这条链上最容易错的部分。
    #[tokio::test]
    async fn the_tab_list_is_read_over_http() {
        let fake = test_support::spawn(
            r#"[{"id":"1","type":"page","title":"Docs","url":"https://example.com/docs",
                 "webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/1"}]"#
                .to_string(),
            "{}".to_string(),
            1,
        )
        .await;

        let tabs = list_tabs(fake.port).await.expect("列表应该读回来");

        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Docs");
        let lines = fake.request_lines();
        assert!(lines[0].starts_with("GET /json/list"), "{}", lines[0]);
    }

    /// 开页面要用 **PUT /json/new**，而且 URL 要经过规范化再放进查询参数。
    ///
    /// Chrome 111 之后 `/json/new` 拒绝 GET，所以方法错了在新版 Chrome 上就是整个功能失效。
    #[tokio::test]
    async fn opening_a_url_puts_a_new_tab_with_the_normalized_url() {
        let fake = test_support::spawn(
            "[]".to_string(),
            r#"{"id":"7","title":"Docs","url":"https://example.com/docs"}"#.to_string(),
            1,
        )
        .await;

        let tab = open_url(fake.port, "https://example.com/docs")
            .await
            .expect("应该开成功");

        assert_eq!(tab.id, "7");
        assert_eq!(tab.url, "https://example.com/docs");
        let lines = fake.request_lines();
        // 端点形状是 `/json/new?<编码后的 url>`，**没有** `url=` 这个参数名 —— 这是 Chrome
        // 自己的格式，写这条测试时我先猜错了一次，所以把真实形状钉在这里。
        assert!(lines[0].starts_with("PUT /json/new?"), "{}", lines[0]);
        assert!(lines[0].contains("example.com"), "{}", lines[0]);
        assert!(!lines[0].contains("url="), "{}", lines[0]);
    }

    /// 端点不存在时要说清**怎么办**，而不是一句 "connection refused"。
    ///
    /// 这条路径是用户第一次用浏览器工具时最可能撞上的：Chrome 没带
    /// `--remote-debugging-port` 起，而那句话是他唯一需要的信息。
    ///
    /// 用 1 号端口而不是"先占一个临时端口再放掉"：放掉之后那个号会立刻被并行跑的别的
    /// 测试的假服务抢走，于是连接**成功**了，这条测试就变成了随机失败。1 号端口需要管理员
    /// 才绑得上，没人会占。
    #[tokio::test]
    async fn a_closed_port_says_how_to_start_chrome() {
        let error = list_tabs(1).await.unwrap_err();

        assert!(error.contains("--remote-debugging-port"), "{}", error);
        assert!(error.contains('1'), "{}", error);
    }

    /// 设了代理也照样直连 127.0.0.1。
    ///
    /// reqwest 默认读 `HTTP_PROXY` / `ALL_PROXY`，而这台机器上就设了一个 —— 上面那条
    /// "连不上要说怎么办"的测试第一次跑出来是"空响应体"，因为请求被送去了代理。生产里
    /// 的后果更重：`open_url` 把目标 URL 放在请求行里，经代理就是一次没人授权过的披露。
    ///
    /// 用 `#[test]` 自己建 runtime：`env_test_guard()` 是同步锁，async 测试里它会跨 `await`。
    #[test]
    fn a_proxy_in_the_environment_does_not_intercept_loopback_cdp() {
        let _guard = crate::services::workspace::env_test_guard();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("测试 runtime");
        runtime.block_on(async {
            let fake = test_support::spawn(
                r#"[{"id":"1","type":"page","title":"Docs","url":"https://example.com/docs"}]"#
                    .to_string(),
                "{}".to_string(),
                1,
            )
            .await;
            // 指向一个没人监听的端口：走代理的话这次请求不可能拿到那份列表
            std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
            std::env::set_var("ALL_PROXY", "http://127.0.0.1:1");

            let tabs = list_tabs(fake.port).await;

            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("ALL_PROXY");
            assert_eq!(tabs.expect("应该直连读到列表").len(), 1);
        });
    }

    fn session(title: &str, url: &str, readable: bool) -> PageSession {
        PageSession {
            tab: BrowserTab {
                id: title.to_string(),
                title: title.to_string(),
                url: url.to_string(),
            },
            ws_url: readable.then(|| "ws://127.0.0.1:9222/devtools/page/X".to_string()),
        }
    }

    /// 开着 DevTools 的页面要留在列表里：把它过滤掉之后，"没有这个页面"和"这个页面
    /// 读不了"会给出同一句错误，而用户的下一步完全不同。
    #[test]
    fn a_page_whose_debugger_is_taken_is_kept_and_marked() {
        let body = r#"[
            {"id":"1","type":"page","title":"Docs","url":"https://example.com/docs",
             "webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/1"},
            {"id":"2","type":"page","title":"Busy","url":"https://example.com/busy"}
        ]"#;

        let sessions = parse_page_sessions(body).unwrap();

        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].ws_url.is_some());
        assert!(sessions[1].ws_url.is_none());
        // 同一份响应经 `parse_page_targets` 出来仍然是两个标签页：读不了不等于不存在
        assert_eq!(parse_page_targets(body).unwrap().len(), 2);
    }

    /// 命中多个就拒绝：读错一页就是把另一个站点的内容交给了模型，而"第一个"只由
    /// Chrome 的返回顺序决定。
    #[test]
    fn reading_needs_exactly_one_matching_page() {
        let open = vec![
            session("Docs A", "https://example.com/a", true),
            session("Docs B", "https://example.com/b", true),
        ];
        let allowlist = vec!["https://example.com".to_string()];

        let ambiguous =
            select_read_target(open.clone(), Some("example.com"), None, &allowlist).unwrap_err();
        assert!(ambiguous.contains("Docs A") && ambiguous.contains("Docs B"));

        let none = select_read_target(open.clone(), Some("/nope"), None, &allowlist).unwrap_err();
        assert!(none.contains("/nope"));

        let chosen = select_read_target(open, Some("/b"), None, &allowlist).unwrap();
        assert_eq!(chosen.tab.url, "https://example.com/b");
    }

    /// 不带筛选条件的调用要拒掉，而不是解析成"反正只有一个"。
    ///
    /// 空条件命中每一个被允许的页面，而"命中多个"的拒绝话术里带着候选页面的标题和 URL ——
    /// 于是一次无参调用就是 `workspace_browser_tabs`，绕开了那个工具自己的授权。
    #[test]
    fn a_read_without_a_filter_is_refused_instead_of_listing_everything() {
        let open = vec![
            session("Docs A", "https://example.com/a", true),
            session("Docs B", "https://example.com/b", true),
        ];

        let error = select_read_target(open, None, None, &["*".to_string()]).unwrap_err();

        assert!(!error.contains("Docs A"), "{}", error);
        assert!(!error.contains("Docs B"), "{}", error);
        assert!(error.contains("url_contains"));
    }

    /// 清单过滤的是**候选**：不在允许 origin 里的页面既不参与匹配，标题也不出现在
    /// 拒绝理由里 —— 否则允许清单只限制"能读到什么"，却不限制"能看见有什么"。
    #[test]
    fn pages_outside_the_allowed_origins_are_neither_read_nor_named() {
        let open = vec![
            session("Bank", "https://bank.example/accounts", true),
            session("Preview", "http://127.0.0.1:1420/", true),
        ];
        let allowlist = vec!["http://127.0.0.1:1420".to_string()];

        let chosen = select_read_target(open.clone(), Some("127.0.0.1"), None, &allowlist).unwrap();
        assert_eq!(chosen.tab.url, "http://127.0.0.1:1420/");

        let refused = select_read_target(open, Some("accounts"), None, &allowlist).unwrap_err();
        assert!(!refused.contains("Bank"));
        assert!(!refused.contains("bank.example"));
        assert!(refused.contains("1 other open page"));
    }

    /// 页面自己的 DevTools 占着调试通道时，说清楚要关掉它，而不是一句 "not found"。
    #[test]
    fn an_unattachable_page_says_why() {
        let error = select_read_target(
            vec![session("Busy", "https://example.com/busy", false)],
            Some("/busy"),
            None,
            &["*".to_string()],
        )
        .unwrap_err();

        assert!(error.contains("Busy"));
        assert!(error.contains("DevTools"));
    }

    /// 截断在页面里做，而且两处都用同一个上限 —— 只在一处写死会让"传回来的"和
    /// "报出来的"对不上。按码位切：`slice` 数 UTF-16 单元，切在星文平面字符中间会留下
    /// 半个代理对，那不是合法 JSON。还要把 `location.href` 带回来，跨 origin 复核靠它。
    #[test]
    fn the_page_side_script_truncates_by_code_point_and_reports_the_url() {
        let expression = page_text_expression(1234);

        assert!(expression.contains("Array.from"));
        assert!(expression.contains("slice(0, 1234)"));
        assert!(expression.contains("points.length > 1234"));
        assert!(expression.contains("chars: points.length"));
        assert!(expression.contains("url: location.href"));
    }

    /// 只连回环上属于这个端口的调试 socket：这是一个由响应内容决定"去连哪儿"的字段。
    #[test]
    fn only_this_ports_loopback_debugger_is_attached_to() {
        assert!(validate_page_ws_url("ws://127.0.0.1:9222/devtools/page/A", 9222).is_ok());
        for hostile in [
            "ws://evil.example/devtools/page/A",
            "ws://127.0.0.1:9333/devtools/page/A",
            "wss://127.0.0.1:9222/devtools/page/A",
            "ws://127.0.0.1:92220/devtools/page/A",
            // 结尾那个 `/` 是这道检查的全部力气：少了它，userinfo 的 `@` 就能占住
            // authority 的位置，真正被连接的主机是 `evil.example`
            "ws://127.0.0.1:9222@evil.example/devtools/page/A",
            "ws://127.0.0.1:9222.evil.example/devtools/page/A",
        ] {
            assert!(
                validate_page_ws_url(hostile, 9222).is_err(),
                "should refuse: {}",
                hostile
            );
        }
    }

    /// 页面在等批准的两分钟里导航走了，就当没读到 —— 而且不说它去了哪儿。
    ///
    /// 调试 socket 绑的是 target，页面导航到别处它照样有效，所以"列出时在允许清单里"
    /// 不等于"读到时还在"。跳转后的那个站点正是没被授权的那个，把它的地址写进返回值
    /// 等于替这次被拒的读取完成了披露。
    #[test]
    fn a_page_that_navigated_after_approval_is_not_read() {
        assert!(
            verify_read_origin("https://example.com", "https://example.com/other/path").is_ok()
        );

        let error =
            verify_read_origin("https://example.com", "https://evil.example/landing").unwrap_err();
        assert!(!error.contains("evil.example"), "{}", error);
        assert!(error.contains("https://example.com"));

        // 页面报了个读不出 origin 的地址（`about:blank`、`chrome-error://`）也算跳走了
        assert!(verify_read_origin("https://example.com", "about:blank").is_err());
    }

    /// 三种失败都不能变成"读到了空文本"：模型会据此断定页面是空的然后往下走。
    #[test]
    fn a_failed_read_is_never_reported_as_an_empty_page() {
        let cdp_error = parse_evaluate_response(
            r#"{"id":1,"error":{"code":-32000,"message":"Target closed"}}"#,
        )
        .unwrap_err();
        assert!(cdp_error.contains("Target closed"));

        let thrown = parse_evaluate_response(
            r#"{"id":1,"result":{"result":{"type":"object"},"exceptionDetails":{"text":"Uncaught"}}}"#,
        )
        .unwrap_err();
        assert!(thrown.contains("Uncaught"));

        assert!(parse_evaluate_response(r#"{"id":1,"result":{}}"#).is_err());
        assert!(parse_evaluate_response("not json").is_err());
        // 没有 URL 就没法复核跨 origin，所以缺它是错误而不是"那就不查了"
        assert!(parse_evaluate_response(
            r#"{"id":1,"result":{"result":{"value":{"text":"hi","chars":2}}}}"#
        )
        .is_err());
    }

    /// 截断了就必须说，而且要说出原本有多长：只给一段掐断的文本会让模型以为读完了。
    #[test]
    fn a_truncated_page_reports_the_length_it_had() {
        let answer = parse_evaluate_response(
            r#"{"id":1,"result":{"result":{"type":"object","value":
               {"url":"https://example.com/docs","text":"abc","chars":50000,"truncated":true}}}}"#,
        )
        .unwrap();

        assert_eq!(answer.text, "abc");
        assert_eq!(answer.url, "https://example.com/docs");
        assert!(answer.truncated);
        assert_eq!(answer.chars, 50_000);
    }

    /// 一个只回答一次的假 CDP 页面 socket。
    ///
    /// 返回它监听的端口。只服务一个连接：这些测试各自只读一页。
    async fn fake_page_debugger(reply: serde_json::Value) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            // 先照着 Chrome 的样子来一条无关事件：回答必须按 id 认，拿第一条消息当结果
            // 会把这条事件解析成"这一页是空的"
            socket
                .send(Message::Text(
                    r#"{"method":"Runtime.executionContextCreated","params":{}}"#.into(),
                ))
                .await
                .unwrap();
            let _request = socket.next().await;
            socket
                .send(Message::Text(reply.to_string().into()))
                .await
                .unwrap();
            // 不主动关：真正的 Chrome 也不会在回答之后立刻关，而读取方不该依赖它关
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        port
    }

    /// 真的连一次 WebSocket 并读回正文。
    ///
    /// 这条路径上此前没有任何自动化覆盖：握手、按 id 认回答、以及"读到的 URL"都只在
    /// 真的连上去之后才发生。
    #[tokio::test]
    async fn the_page_text_comes_back_over_a_real_websocket() {
        let port = fake_page_debugger(serde_json::json!({
            "id": 1,
            "result": { "result": { "type": "object", "value": {
                "url": "http://127.0.0.1:1420/index.html",
                "text": "Preview is up",
                "chars": 13,
                "truncated": false
            }}}
        }))
        .await;

        let page = read_page_text(
            &format!("ws://127.0.0.1:{}/devtools/page/1", port),
            port,
            100,
        )
        .await
        .unwrap();

        assert_eq!(page.text, "Preview is up");
        assert_eq!(page.url, "http://127.0.0.1:1420/index.html");
        assert!(!page.truncated);
    }

    /// 连得上但关掉了连接，要说"在回答之前就关了"，而不是报成超时。
    #[tokio::test]
    async fn a_socket_that_closes_without_answering_says_so() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _request = socket.next().await;
            let _ = socket.close(None).await;
        });

        let error = read_page_text(
            &format!("ws://127.0.0.1:{}/devtools/page/1", port),
            port,
            100,
        )
        .await
        .unwrap_err();

        assert!(error.contains("closed"), "{}", error);
        assert!(!error.contains("did not answer within"), "{}", error);
    }
}
