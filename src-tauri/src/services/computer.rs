//! 桌面观察面：枚举可见的顶层窗口。
//!
//! 这是 computer use 的第一片，而且**只有读**。为什么先做这个、而不是先做截屏或
//! 输入注入：
//!
//! - 截屏现在没有用。`ChatMessage.content` 是 `String`，LLM 客户端还没有多模态内容，
//!   截出来的图模型看不到。发一个模型读不了的工具就是那种"什么都不做的控件"，
//!   AGENTS.md 里明确禁止。
//! - 输入注入（点击、按键）撤不回，而且比导航更狠：它能点掉任何一个确认框。它需要
//!   的授权模型比"允许操作桌面"细得多（参考实现是按应用逐个批准），先把授权和记录
//!   的形状在只读能力上验证一遍，代价小得多。
//!
//! 窗口标题本身就是要保护的东西：文档名、网页标题、聊天窗口的对方名字都在里面。
//! 所以这里的授权是**按应用**的白名单，而且它过滤的是**结果**，不只是决定工具存不存在
//! —— 浏览器 tabs 工具当初犯的正是后一个错误。

use serde::Serialize;

/// 一个可见的顶层窗口。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DesktopWindow {
    /// 窗口标题
    pub title: String,
    /// 进程的可执行文件名（`code.exe`、`chrome.exe`），用于和白名单比对
    pub app: String,
    /// 屏幕坐标 (x, y, width, height)
    pub bounds: (i32, i32, i32, i32),
    /// 是否是当前前台窗口
    pub foreground: bool,
}

/// 把用户写的应用名收成比对用的形式。
///
/// 去掉路径、去掉 `.exe`、转小写：用户会写 `Code.exe`、`code`、甚至
/// `C:\...\Code.exe`，这三种都该命中同一个应用。不做这层归一化的话，白名单会
/// "看起来配了但从不命中" —— 比空白名单更糟，因为它让人以为已经授权了。
pub fn normalize_app_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    let file = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed).trim();
    // 先转小写再去扩展名：反过来的话 `Chrome.EXE` 会留着 `.exe` 变成 `chrome.exe`，
    // 而枚举出来的是 `chrome` —— 白名单里写大写扩展名的那一条永远不会命中，
    // 也就是这个函数本来要避免的那种"配了却不生效"。
    let lower = file.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// 进程名读不出来时用的占位。
///
/// 单独成一个常量，因为它在授权判断里有特殊地位：见 `app_allowed`。
pub const UNKNOWN_APP: &str = "unknown";

/// 这个应用是否在白名单里。空白名单一律不允许。
///
/// 和 origin 白名单同一个判断：空列表不读成"没配所以全放"。事后看，默认全放的
/// 列表和用户真的批准过的列表长得一模一样。
///
/// **`unknown` 永远不匹配，`*` 也不例外。** 拿不到进程名最常见的原因是那是个提权进程
/// （非提权的 Agent IDE 打不开它的句柄），而提权窗口恰恰是更敏感的那批。一个身份不明的
/// 窗口没法被归到任何"已批准的应用"名下，所以它不披露 —— 这样"读不到就不披露"才是
/// 一条真的不变量，而不是一句好听的话。代价是真有个叫 `unknown.exe` 的进程也看不到，
/// 这个代价可以接受。
pub fn app_allowed(app: &str, allowlist: &[String]) -> bool {
    let target = normalize_app_name(app);
    if target.is_empty() || target == UNKNOWN_APP {
        return false;
    }
    allowlist.iter().any(|entry| {
        let entry = entry.trim();
        entry == "*" || normalize_app_name(entry) == target
    })
}

/// 按白名单过滤，返回 (放行的窗口, 被挡掉的数量)。
///
/// 被挡掉的数量要一起返回：模型和记录都需要知道"这不是全部"，否则一次被过滤过的
/// 列表看起来就是桌面的全貌。
pub fn filter_windows(
    windows: Vec<DesktopWindow>,
    allowlist: &[String],
) -> (Vec<DesktopWindow>, usize) {
    let total = windows.len();
    let allowed: Vec<DesktopWindow> = windows
        .into_iter()
        .filter(|window| app_allowed(&window.app, allowlist))
        .collect();
    let hidden = total - allowed.len();
    (allowed, hidden)
}

/// 给模型看的文本。
///
/// 明说被挡掉了几个：模型据此知道自己看到的是子集，而不是"桌面上只有这些"。
pub fn format_windows(windows: &[DesktopWindow], hidden: usize) -> String {
    if windows.is_empty() {
        return format!(
            "No window matched the allowed apps ({} window(s) hidden by the allow list).",
            hidden
        );
    }
    let mut out = String::new();
    for window in windows {
        let (x, y, width, height) = window.bounds;
        out.push_str(&format!(
            "{}{} [{}] {}x{} at ({},{})\n",
            if window.foreground { "* " } else { "  " },
            if window.title.is_empty() {
                "(untitled)"
            } else {
                window.title.as_str()
            },
            window.app,
            width,
            height,
            x,
            y
        ));
    }
    out.push_str(&format!(
        "\n{} window(s) shown, {} hidden by the allow list. '*' marks the foreground window.",
        windows.len(),
        hidden
    ));
    out
}

/// 这次披露涉及哪些应用，去重后按出现顺序。给记录用。
pub fn disclosed_apps(windows: &[DesktopWindow]) -> Vec<String> {
    let mut apps: Vec<String> = Vec::new();
    for window in windows {
        if !apps.contains(&window.app) {
            apps.push(window.app.clone());
        }
    }
    apps
}

#[cfg(windows)]
mod platform {
    use super::{DesktopWindow, UNKNOWN_APP};
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, MAX_PATH, RECT, TRUE};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    /// `EnumWindows` 的回调把结果攒进这里。
    ///
    /// 连 `HWND` 一起留着：截图要对着**这一次枚举里**的那个句柄画，不能事后再按标题
    /// 找一遍 —— 中间窗口可能已经关了或换了标题，那时"按标题再找"会截到另一个窗口。
    struct Collector {
        windows: Vec<(DesktopWindow, HWND)>,
        foreground: HWND,
    }

    /// 枚举可见的顶层窗口。
    ///
    /// 跳过没有标题的窗口：Windows 上有大量不可见的消息窗口和零尺寸的辅助窗口，
    /// 它们对用户没有意义，混进列表只会淹没真正的窗口。
    pub fn list_windows() -> Result<Vec<DesktopWindow>, String> {
        Ok(list_windows_with_handles()?
            .into_iter()
            .map(|(window, _)| window)
            .collect())
    }

    /// 枚举可见的顶层窗口，连句柄一起返回。只给截图用。
    pub fn list_windows_with_handles() -> Result<Vec<(DesktopWindow, HWND)>, String> {
        let mut collector = Collector {
            windows: Vec::new(),
            foreground: unsafe { GetForegroundWindow() },
        };
        // SAFETY: `EnumWindows` 在返回前同步调用回调，所以这个指针在调用期间一直有效；
        // 回调里不 panic（只做取字符串和 push），因此不会跨 FFI 边界展开。
        let ok = unsafe {
            EnumWindows(
                Some(enum_callback),
                &mut collector as *mut Collector as LPARAM,
            )
        };
        if ok != TRUE {
            // 回调提前返回 FALSE 也会让它返回 FALSE，但我们从不这样做，所以这里
            // 只可能是真的失败了
            return Err("EnumWindows failed while listing desktop windows.".to_string());
        }
        Ok(collector.windows)
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, param: LPARAM) -> BOOL {
        let collector = &mut *(param as *mut Collector);
        if IsWindowVisible(hwnd) != TRUE {
            return TRUE;
        }
        let title = window_title(hwnd);
        if title.trim().is_empty() {
            return TRUE;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) != TRUE {
            return TRUE;
        }
        collector.windows.push((
            DesktopWindow {
                title,
                app: window_app(hwnd),
                bounds: (
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                ),
                foreground: hwnd == collector.foreground,
            },
            hwnd,
        ));
        TRUE
    }

    unsafe fn window_title(hwnd: HWND) -> String {
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return String::new();
        }
        // +1 给结尾的 NUL：GetWindowTextW 要求缓冲区能放下它，否则会截掉最后一个字符
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if copied <= 0 {
            return String::new();
        }
        std::ffi::OsString::from_wide(&buffer[..copied as usize])
            .to_string_lossy()
            .into_owned()
    }

    /// 取窗口所属进程的可执行文件名。
    ///
    /// 拿不到时返回 `unknown` 而不是跳过这个窗口：白名单按应用比对，`unknown`
    /// 不会命中任何条目，于是这个窗口自然被挡在外面 —— 失败的方向是"不披露"。
    unsafe fn window_app(hwnd: HWND) -> String {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return UNKNOWN_APP.to_string();
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return UNKNOWN_APP.to_string();
        }
        let mut buffer = vec![0u16; MAX_PATH as usize];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        windows_sys::Win32::Foundation::CloseHandle(handle);
        if ok != TRUE || size == 0 {
            return UNKNOWN_APP.to_string();
        }
        let full = std::ffi::OsString::from_wide(&buffer[..size as usize])
            .to_string_lossy()
            .into_owned();
        full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::DesktopWindow;

    /// 非 Windows 平台上没有实现。
    ///
    /// 返回错误而不是空列表：空列表会被读成"桌面上没有窗口"，那是假话。工具本身
    /// 在这些平台上也不通告（见 `workspace_tools::handles`）。
    pub fn list_windows() -> Result<Vec<DesktopWindow>, String> {
        Err("Listing desktop windows is only implemented on Windows.".to_string())
    }
}

pub use platform::list_windows;
#[cfg(windows)]
pub use platform::list_windows_with_handles;

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, app: &str, foreground: bool) -> DesktopWindow {
        DesktopWindow {
            title: title.to_string(),
            app: app.to_string(),
            bounds: (0, 0, 800, 600),
            foreground,
        }
    }

    #[test]
    fn app_names_are_compared_without_path_case_or_extension() {
        // 用户会用这几种写法里的任意一种。`Chrome.EXE` 这一条曾经不命中：归一化先去
        // 扩展名再转小写，于是大写的 `.EXE` 留在了名字里 —— 白名单看着配了却从不生效。
        for entry in [
            "Code.exe",
            "code",
            "C:\\Program Files\\Code.exe",
            "CODE.EXE",
            "  \"code.Exe\"  ",
        ] {
            assert!(
                app_allowed("code.exe", &[entry.to_string()]),
                "entry {} should match",
                entry
            );
        }
        assert!(!app_allowed("chrome.exe", &["code".to_string()]));
    }

    #[test]
    fn an_empty_allow_list_allows_nothing_and_a_star_allows_everything() {
        // 空列表不读成"没配就全放"：事后看，默认全放和用户批准过长得一样
        assert!(!app_allowed("code.exe", &[]));
        assert!(app_allowed("anything.exe", &["*".to_string()]));
    }

    /// 身份不明的窗口不披露，`*` 也不例外。
    ///
    /// 上一版的断言写的是 `!app_allowed("", ...)` —— 空串是 `window_app` 永远不会返回的
    /// 值，所以那条测试测的是一个不可达分支，而真正会出现的 `unknown` 当时是**放行**的。
    /// 拿不到进程名最常见的原因是提权进程，而那批窗口更敏感。
    #[test]
    fn a_window_whose_app_cannot_be_read_is_never_disclosed() {
        assert!(!app_allowed(UNKNOWN_APP, &["*".to_string()]));
        // 连显式写进清单也不放行：那一栏是"已批准的应用"，不是一个可以点名的桶
        assert!(!app_allowed(UNKNOWN_APP, &[UNKNOWN_APP.to_string()]));

        let windows = vec![
            window("Elevated tool", UNKNOWN_APP, false),
            window("main.rs", "Code.exe", true),
        ];
        let (allowed, hidden) = filter_windows(windows, &["*".to_string()]);
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0].app, "Code.exe");
        assert_eq!(hidden, 1);
    }

    #[test]
    fn filtering_reports_how_many_windows_it_hid() {
        let windows = vec![
            window("main.rs - agent-ide", "Code.exe", true),
            window("Bank statement", "chrome.exe", false),
            window("Notes", "notepad.exe", false),
        ];

        let (allowed, hidden) = filter_windows(windows, &["code".to_string()]);

        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0].app, "Code.exe");
        // 数量必须回报：一次过滤过的列表不能看起来像桌面全貌
        assert_eq!(hidden, 2);
    }

    #[test]
    fn the_text_for_the_model_says_it_is_a_subset() {
        let allowed = vec![window("main.rs", "Code.exe", true)];
        let text = format_windows(&allowed, 4);

        assert!(text.contains("main.rs"), "{}", text);
        assert!(text.contains("* "), "foreground marker missing: {}", text);
        assert!(text.contains("4 hidden"), "{}", text);
    }

    #[test]
    fn an_empty_result_still_says_how_many_were_hidden() {
        // 否则模型会以为桌面上什么都没开，而不是"你没授权我看"
        let text = format_windows(&[], 7);
        assert!(text.contains("7 window(s) hidden"), "{}", text);
    }

    #[test]
    fn disclosed_apps_are_deduplicated_in_order() {
        let windows = vec![
            window("a", "Code.exe", false),
            window("b", "chrome.exe", false),
            window("c", "Code.exe", false),
        ];
        assert_eq!(disclosed_apps(&windows), vec!["Code.exe", "chrome.exe"]);
    }
}
