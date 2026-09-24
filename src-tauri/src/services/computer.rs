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
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
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
        let Some(window) = window_from_handle(hwnd, collector.foreground) else {
            return TRUE;
        };
        // 跳过没有标题的窗口：Windows 上有大量不可见的消息窗口和零尺寸的辅助窗口
        if window.title.trim().is_empty() {
            return TRUE;
        }
        collector.windows.push((window, hwnd));
        TRUE
    }

    /// 重新读一个句柄现在的样子。窗口已经关掉（或句柄不再有效）时返回 `None`。
    ///
    /// 存在的理由：截图要"先问人、再动手"，而 `HWND` 是裸指针、不是 `Send`，过不了
    /// `await`。过得去的是它的**值**（`isize`），所以批准之后拿值换回句柄，再用这个
    /// 函数确认它还活着、还属于同一个应用 —— 而不是按标题重新找一遍。按标题找会
    /// 截到另一个同名窗口，而标题是被披露方自己就能改的东西（网页标题即窗口标题）。
    pub fn describe_window(handle: isize) -> Option<DesktopWindow> {
        let hwnd = handle as HWND;
        // SAFETY: 只读 API，且 `IsWindow` 先确认句柄有效；无效句柄会被这里挡住，
        // 而不是传给后面的 Get* 调用。
        unsafe {
            if IsWindow(hwnd) != TRUE {
                return None;
            }
            window_from_handle(hwnd, GetForegroundWindow())
        }
    }

    /// 这个句柄现在属于哪个进程。`None` = 句柄已经无效。
    ///
    /// 和 `describe_window` 分开：应用名会重名（同一个 Chrome 的两个窗口），pid 才是
    /// "还是不是那一个窗口"里唯一不会被标题或应用名糊弄过去的一半 —— Win32 在窗口销毁
    /// 之后会把句柄回收给新窗口，回收给**同一应用**时只比应用名是看不出来的。
    pub fn window_pid(handle: isize) -> Option<u32> {
        let hwnd = handle as HWND;
        // SAFETY: 同 `describe_window`，先确认句柄有效再问它的进程。
        unsafe {
            if IsWindow(hwnd) != TRUE {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            (pid != 0).then_some(pid)
        }
    }

    /// 读一个句柄的标题、应用、位置。标题为空或取不到矩形时返回 `None`。
    ///
    /// 枚举回调和 `describe_window` 共用这一处：`bounds` 的算法和前台判定各写两份的话，
    /// 将来改一处必然漏另一处。
    unsafe fn window_from_handle(hwnd: HWND, foreground: HWND) -> Option<DesktopWindow> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) != TRUE {
            return None;
        }
        Some(DesktopWindow {
            title: window_title(hwnd),
            app: window_app(hwnd),
            bounds: (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
            foreground: hwnd == foreground,
        })
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

    /// 窗口类名。
    ///
    /// 身份检查里最后一块能拿到的东西。应用名 + pid 相同的情况下，句柄被**同一进程**里
    /// 另一个窗口回收是查不出来的（Chrome 关一个窗口开一个提示气泡就是这样）——而类名
    /// 在那种情况下通常不同（`Chrome_WidgetWin_1` 对 `tooltips_class32`）。它不是万能的：
    /// 同一类的两个兄弟窗口照样分不开，所以这只是把那道缝收窄，不是焊死。
    pub fn window_class(handle: isize) -> Option<String> {
        // SAFETY: 句柄只用来查，越界由 `IsWindow` 挡在前面；缓冲区是本地的。
        unsafe {
            let hwnd = handle as HWND;
            if IsWindow(hwnd) != TRUE {
                return None;
            }
            let mut buffer = vec![0u16; 256];
            let length = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            if length <= 0 {
                return None;
            }
            Some(
                std::ffi::OsString::from_wide(&buffer[..length as usize])
                    .to_string_lossy()
                    .into_owned(),
            )
        }
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

    /// 非 Windows 上没有句柄可言。
    pub fn describe_window(_handle: isize) -> Option<DesktopWindow> {
        None
    }

    pub fn window_pid(_handle: isize) -> Option<u32> {
        None
    }

    /// 非 Windows 上没有窗口类名。
    ///
    /// 必须有这一份：调用方（`workspace_tools` 的点击/滚动路径）是平台无关的代码，只把
    /// 类名当作身份校验的一项传给 `ApprovedWindow::verify`。那边同一次校验里的
    /// `describe_window` 在这些平台上返回 `None`，所以校验会先以"窗口已经不在了"拒绝，
    /// 类名取不到不会让任何一下发出去。
    pub fn window_class(_handle: isize) -> Option<String> {
        None
    }
}

#[cfg(windows)]
pub use platform::list_windows_with_handles;
pub use platform::{describe_window, list_windows, window_class, window_pid};

/// 造一个真实窗口，给那些非得有窗口才测得到的东西用。
///
/// 截图和点击都是"对着一个真窗口做 Win32 调用"，纯函数覆盖不到：GDI 的 stride 算错、
/// `SendInput` 根本没发出去，在单元测试里都是绿的。两边共用这一份而不是各写一个 ——
/// 一份跑着、一份烂掉是这类测试最常见的结局。
#[cfg(all(test, windows))]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
        RegisterClassW, TranslateMessage, CS_DBLCLKS, MSG, PM_REMOVE, WM_LBUTTONDBLCLK,
        WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    static CLICKS: AtomicU32 = AtomicU32::new(0);
    static RIGHT_CLICKS: AtomicU32 = AtomicU32::new(0);
    static DOUBLE_CLICKS: AtomicU32 = AtomicU32::new(0);
    static WHEEL_EVENTS: AtomicU32 = AtomicU32::new(0);
    static WHEEL_DELTA: AtomicI32 = AtomicI32::new(0);
    static CLIENT_X: AtomicI32 = AtomicI32::new(-1);
    static CLIENT_Y: AtomicI32 = AtomicI32::new(-1);

    /// 同一时刻只准有一个测试窗口。
    ///
    /// 上面那几个计数器是进程级的，而 `cargo test` 默认并行：两条测试各开一个窗口时，一条
    /// 读到的"收到几次点击"里混着另一条发出去的那几下。这类失败是间歇性的，而间歇性失败
    /// 最后都会被当成噪声忽略掉。
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 记下收到的是哪一种输入，以及左键落点的客户区坐标。
    ///
    /// 四种消息各记一个计数器而不是合成一个"收到过输入"：右键被发成左键、双击只到了一下、
    /// 滚轮方向反了，都只有分开数才看得出来，而它们在真实使用里唯一的发现方式是人盯着屏幕。
    ///
    /// `lParam` 低 16 位是 x、高 16 位是 y，都是**有符号**的；滚轮的格数在 `wParam` 高 16 位，
    /// 同样有符号。
    unsafe extern "system" fn record_clicks(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_LBUTTONDOWN => {
                CLICKS.fetch_add(1, Ordering::SeqCst);
                CLIENT_X.store((lparam & 0xFFFF) as i16 as i32, Ordering::SeqCst);
                CLIENT_Y.store(((lparam >> 16) & 0xFFFF) as i16 as i32, Ordering::SeqCst);
            }
            WM_RBUTTONDOWN => {
                RIGHT_CLICKS.fetch_add(1, Ordering::SeqCst);
            }
            // 真实使用里，只有窗口类带了 `CS_DBLCLKS` 才会收到这条：Win32 不会自己把两下
            // 按键"合成"成双击事件，是窗口类要求的。（直接 `SendMessageW` 送这条消息不受
            // 这个限制，所以下面那条计数测试验的是"数得对"，样式是否真的设上了另有断言。）
            WM_LBUTTONDBLCLK => {
                DOUBLE_CLICKS.fetch_add(1, Ordering::SeqCst);
            }
            WM_MOUSEWHEEL => {
                WHEEL_EVENTS.fetch_add(1, Ordering::SeqCst);
                WHEEL_DELTA.store(((wparam >> 16) & 0xFFFF) as i16 as i32, Ordering::SeqCst);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 一个活着的测试窗口。
    ///
    /// 消息泵在自己的线程上，因为 Win32 的窗口是线程亲和的 —— 而这恰好也是生产里的形状：
    /// 被截图、被点击的窗口属于别的线程、别的进程。
    pub struct TestWindow {
        pub handle: isize,
        stop: Arc<AtomicBool>,
        pump: Option<std::thread::JoinHandle<()>>,
        /// 活着就占着那把锁，`Drop` 时还回去
        _one_at_a_time: std::sync::MutexGuard<'static, ()>,
    }

    impl TestWindow {
        /// 开一个可见窗口。创建失败就 panic：没有被测对象的时候，这些测试不该悄悄变绿。
        pub fn open(title: &str, width: i32, height: i32) -> Self {
            // 中毒也继续：上一条测试的断言 panic 不该让后面每一条都变成"窗口开不出来"
            let guard = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            CLICKS.store(0, Ordering::SeqCst);
            RIGHT_CLICKS.store(0, Ordering::SeqCst);
            DOUBLE_CLICKS.store(0, Ordering::SeqCst);
            WHEEL_EVENTS.store(0, Ordering::SeqCst);
            WHEEL_DELTA.store(0, Ordering::SeqCst);
            CLIENT_X.store(-1, Ordering::SeqCst);
            CLIENT_Y.store(-1, Ordering::SeqCst);
            let class_name = wide("AgentIdeTestWindow");
            let window_title = wide(title);
            let stop = Arc::new(AtomicBool::new(false));
            let pump_stop = stop.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<isize>();
            let pump = std::thread::spawn(move || {
                // SAFETY: 类名和标题是本地的、以 0 结尾的宽字符串；回调是本模块里的函数。
                let hwnd = unsafe {
                    let class = WNDCLASSW {
                        // `CS_DBLCLKS`：没有它，窗口永远收不到 `WM_LBUTTONDBLCLK`，双击
                        // 那条测试就会变成"两次单击也算过"。这也是真实应用里双击成不成立
                        // 的条件，所以测试窗口要和它们一样。
                        style: CS_DBLCLKS,
                        lpfnWndProc: Some(record_clicks),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: GetModuleHandleW(std::ptr::null()),
                        hIcon: std::ptr::null_mut(),
                        hCursor: std::ptr::null_mut(),
                        hbrBackground: std::ptr::null_mut(),
                        lpszMenuName: std::ptr::null(),
                        lpszClassName: class_name.as_ptr(),
                    };
                    // 同一进程里注册第二次会失败，但不影响创建 —— 类已经在了
                    RegisterClassW(&class);
                    CreateWindowExW(
                        0,
                        class_name.as_ptr(),
                        window_title.as_ptr(),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        120,
                        120,
                        width,
                        height,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        GetModuleHandleW(std::ptr::null()),
                        std::ptr::null(),
                    )
                };
                let _ = sender.send(hwnd as isize);
                if hwnd.is_null() {
                    return;
                }
                // 一直泵到被要求停：置前、绘制、那一下点击各自都要走消息队列，只泵一次不够
                while !pump_stop.load(Ordering::SeqCst) {
                    let mut message = MSG {
                        hwnd: std::ptr::null_mut(),
                        message: 0,
                        wParam: 0,
                        lParam: 0,
                        time: 0,
                        pt: POINT { x: 0, y: 0 },
                    };
                    // SAFETY: `message` 是本地变量，窗口属于这个线程。
                    while unsafe {
                        PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE)
                    } != 0
                    {
                        unsafe {
                            TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // SAFETY: 销毁这个线程自己创建的窗口。
                unsafe { DestroyWindow(hwnd) };
            });
            let handle = receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("窗口线程应该报出句柄");
            assert_ne!(handle, 0, "创建测试窗口失败，这条测试就没有被测对象了");
            Self {
                handle,
                stop,
                pump: Some(pump),
                _one_at_a_time: guard,
            }
        }

        pub fn clicks() -> u32 {
            CLICKS.load(Ordering::SeqCst)
        }

        pub fn right_clicks() -> u32 {
            RIGHT_CLICKS.load(Ordering::SeqCst)
        }

        pub fn double_clicks() -> u32 {
            DOUBLE_CLICKS.load(Ordering::SeqCst)
        }

        pub fn wheel_events() -> u32 {
            WHEEL_EVENTS.load(Ordering::SeqCst)
        }

        /// 最后一条滚轮消息带的格数（`WHEEL_DELTA` 的倍数，正数向上）。
        pub fn wheel_delta() -> i32 {
            WHEEL_DELTA.load(Ordering::SeqCst)
        }

        pub fn last_click() -> (i32, i32) {
            (
                CLIENT_X.load(Ordering::SeqCst),
                CLIENT_Y.load(Ordering::SeqCst),
            )
        }
    }

    /// 靠 `Drop` 收尾，而不是在测试末尾 join：断言 panic 时那一行根本执行不到，窗口会活过
    /// 这条测试 —— 而下一条测试可能正要按标题找窗口。
    impl Drop for TestWindow {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(pump) = self.pump.take() {
                let _ = pump.join();
            }
        }
    }
}

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

    /// 测试窗口自己得先数得对。
    ///
    /// `TestWindow` 的那几个计数器是注入测试唯一的证据来源，而注入测试**只在这个会话肯把
    /// 前台交出来的时候**才会走到"收到了"那一支（从后台终端跑 `cargo test` 走的是"拒绝"）。
    /// 也就是说：如果滚轮的格数解错了一位、或者双击记到了左键那个计数器上，注入测试会一直
    /// 绿着，永远发现不了。所以这里直接把消息发给窗口，不经过 `SendInput` —— 这一条在任何
    /// 会话里都跑得到，它保证的是"那几个计数器可信"，而注入测试保证的是"输入真的到得了"。
    #[cfg(windows)]
    #[test]
    fn the_test_window_counts_each_kind_of_mouse_input_separately() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClassLongPtrW, SendMessageW, CS_DBLCLKS, GCL_STYLE, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
        };
        let window = test_support::TestWindow::open("Agent IDE message test", 320, 240);
        let hwnd = window.handle as windows_sys::Win32::Foundation::HWND;
        // 客户区 (40, 30)：低 16 位是 x、高 16 位是 y
        let point = (30_isize << 16) | 40;

        // 样式要在这里查一次。下面那几条 `SendMessageW` 不受 `CS_DBLCLKS` 约束，所以把
        // `style` 改回 0 也照样绿 —— 而双击能不能成立**全靠**这个样式，那条断言只在肯交出
        // 前台的会话里才跑得到。这一行让"样式没了"在任何会话里都会红。
        // SAFETY: 句柄来自上面那个还活着的窗口。
        let style = unsafe { GetClassLongPtrW(hwnd, GCL_STYLE) };
        assert_ne!(
            style as u32 & CS_DBLCLKS,
            0,
            "测试窗口类没带 CS_DBLCLKS，双击那条测试验的就不是真实条件了"
        );

        // SAFETY: 句柄同上；`SendMessageW` 会等到消息被处理完才返回，所以下面的断言看到的
        // 一定是处理之后的状态。
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, point);
            SendMessageW(hwnd, WM_RBUTTONDOWN, 0, point);
            SendMessageW(hwnd, WM_LBUTTONDBLCLK, 0, point);
            // 向下两格：格数在 wParam 高 16 位，负数
            let wheel = ((-240_i16) as u16 as usize) << 16;
            SendMessageW(hwnd, WM_MOUSEWHEEL, wheel, point);
        }

        assert_eq!(test_support::TestWindow::clicks(), 1);
        assert_eq!(test_support::TestWindow::right_clicks(), 1);
        assert_eq!(test_support::TestWindow::double_clicks(), 1);
        assert_eq!(test_support::TestWindow::wheel_events(), 1);
        // 符号必须还原成负数：按无符号读会得到 65296，方向也就反了
        assert_eq!(test_support::TestWindow::wheel_delta(), -240);
        assert_eq!(test_support::TestWindow::last_click(), (40, 30));
    }
}
