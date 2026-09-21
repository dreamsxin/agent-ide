//! 窗口截图：一次只截**一个**窗口，且这个窗口的应用必须在截图白名单里。
//!
//! 为什么不做全屏抓取：全屏没有任何"用户同意过的范围"能约束它 —— 白名单说的是
//! "允许看这个应用"，而一张全屏图会把旁边所有窗口一起交出去。所以这里的入口只接受
//! "哪个应用 / 标题含什么"，命中多个就拒绝，让模型自己缩小范围。

use crate::services::computer::{app_allowed, DesktopWindow};

/// 一次截图的像素上限（宽 × 高）。
///
/// 4 MiB 的单图上限是按**编码后**的字节算的，而 PNG 的压缩率取决于内容：一张纯色的
/// 4K 截图只有几十 KB，一张满是文本和渐变的同尺寸截图能到十几 MB。先按像素拦一道，
/// 编码之后再按字节拦一道 —— 只靠后者意味着先把一张 8000 万像素的位图搬进内存。
/// 400 万像素装得下 2560×1440，超过就拒绝而不是自动缩放：缩放会让"图上写的字"
/// 变成不可读的糊块，而模型不会告诉你它其实没看清。
pub const MAX_CAPTURE_PIXELS: u64 = 4_000_000;

/// 一次截图的结果：截到的那个窗口 + 已经编码好的 PNG。
///
/// 元信息复用 `CaptureTarget` 而不是再抄一遍四个字段：抄两份的话，将来给身份加一样
/// 东西就要改两处，而漏掉的那一处不会报错。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCapture {
    pub target: CaptureTarget,
    pub png: Vec<u8>,
}

/// 这个窗口是不是模型要找的那个。
///
/// 两个筛选条件都可以省略，但**不能都省略**（那等于"随便截一个"）—— 这一条由
/// `select_capture_target` 保证，因为"随便截一个"在多窗口桌面上就是抽奖。
pub fn matches_target(
    window: &DesktopWindow,
    app_filter: Option<&str>,
    title_contains: Option<&str>,
) -> bool {
    if let Some(app) = app_filter {
        // 两边都要归一化。枚举给出的是 `chrome.exe`（带扩展名、大小写照抄系统），
        // 只归一化模型给的那一侧，`chrome` 和 `chrome.exe` 都对不上 —— 这个筛选条件
        // 就成了一个永远不命中的死控件，而且失败方向是"拒绝"，不会有人报错。
        if crate::services::computer::normalize_app_name(app)
            != crate::services::computer::normalize_app_name(&window.app)
        {
            return false;
        }
    }
    if let Some(needle) = title_contains {
        if !window
            .title
            .to_lowercase()
            .contains(&needle.to_lowercase().trim().to_string())
        {
            return false;
        }
    }
    true
}

/// 从枚举结果里挑出唯一一个可截的窗口。
///
/// 三种拒绝都要说清下一步：没有筛选条件（先看窗口列表）、一个都没命中（换条件）、
/// 命中多个（列出候选，让模型缩小）。含糊时**不猜**：截错窗口是不可撤回的披露。
pub fn select_capture_target(
    windows: &[DesktopWindow],
    app_filter: Option<&str>,
    title_contains: Option<&str>,
    allowlist: &[String],
) -> Result<DesktopWindow, String> {
    if app_filter.is_none() && title_contains.map(|t| t.trim().is_empty()).unwrap_or(true) {
        return Err(
            "Name the window to capture: pass app and/or title_contains. List windows first if you do not know what is open."
                .to_string(),
        );
    }
    let allowed: Vec<&DesktopWindow> = windows
        .iter()
        .filter(|window| app_allowed(&window.app, allowlist))
        .collect();
    let matched: Vec<&DesktopWindow> = allowed
        .iter()
        .copied()
        .filter(|window| matches_target(window, app_filter, title_contains))
        .collect();

    match matched.len() {
        0 => Err(format!(
            "No capturable window matched. {} window(s) are inside the capture allow list.",
            allowed.len()
        )),
        1 => Ok(matched[0].clone()),
        _ => {
            let candidates = matched
                .iter()
                .map(|window| format!("{} ({})", window.title, window.app))
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!(
                "{} windows matched, so nothing was captured. Narrow it down: {}",
                matched.len(),
                candidates
            ))
        }
    }
}

/// 像素数够不够小。溢出用饱和乘法：宽高来自 Win32，理论上不可能大到溢出，但
/// "理论上不可能"在这一层不值得赌。
pub fn check_capture_pixels(width: u32, height: u32) -> Result<(), String> {
    let pixels = (width as u64).saturating_mul(height as u64);
    if pixels == 0 {
        return Err("The window has no visible area to capture.".to_string());
    }
    if pixels > MAX_CAPTURE_PIXELS {
        return Err(format!(
            "That window is {}x{} = {} pixels, past the {} pixel capture limit. Capture a smaller window.",
            width, height, pixels, MAX_CAPTURE_PIXELS
        ));
    }
    Ok(())
}

/// 把 RGBA 像素编码成 PNG。
///
/// 用 `png` crate 而不是手写：PNG 要 zlib、CRC32 和逐行过滤器，手写的量级和当初那个
/// 二十行的 base64 完全不同。BMP 能手写，但它不在 provider 认的媒体类型交集里。
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if rgba.len() != expected {
        return Err(format!(
            "Capture buffer is {} bytes but {}x{} RGBA needs {}.",
            rgba.len(),
            width,
            height,
            expected
        ));
    }
    let mut out: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("PNG header: {}", error))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| format!("PNG data: {}", error))?;
        writer
            .finish()
            .map_err(|error| format!("PNG finish: {}", error))?;
    }
    Ok(out)
}

/// 已经选定、但还没有截下来的目标。
///
/// 单独一个类型是因为"截哪个窗口"和"把它截下来"之间现在插着一次人工批准：批准框要
/// 说得出**具体哪个窗口**，而那句话必须来自选择的结果，不能来自模型给的筛选条件 ——
/// 模型写 `app: "chrome"`，命中的可能是任何一个 Chrome 窗口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTarget {
    pub title: String,
    pub app: String,
    pub width: u32,
    pub height: u32,
}

impl CaptureTarget {
    pub fn from_window(window: &DesktopWindow) -> Self {
        Self {
            title: window.title.clone(),
            app: window.app.clone(),
            width: window.bounds.2.max(0) as u32,
            height: window.bounds.3.max(0) as u32,
        }
    }

    /// 给批准框看的一行。
    pub fn describe(&self) -> String {
        format!(
            "{} ({}), {}x{} pixels",
            self.title, self.app, self.width, self.height
        )
    }
}

/// 用户批准过的那个窗口：选中时的样子 + 认得出它的两样证据。
///
/// 带着句柄值（`isize`）而不是只带筛选条件，是这套流程的关键。`HWND` 是裸指针、不是
/// `Send`，过不了 `await`；但它的**值**过得去，而窗口的身份就是这个值。批准之后按
/// 标题重新找一遍是不行的 —— 标题恰恰是被披露方自己能改的东西（网页标题就是窗口
/// 标题），同一个筛选条件在两秒之后可以命中另一个窗口。
#[derive(Clone, Debug)]
pub struct ApprovedWindow {
    pub target: CaptureTarget,
    handle: isize,
    /// 批准那一刻这个窗口属于哪个进程。取不到时是 `None`（降级为只比应用名）。
    pid: Option<u32>,
}

impl ApprovedWindow {
    pub fn handle(&self) -> isize {
        self.handle
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// 从一帧截图记下来的身份重建它，用来在**之后**的动作（点击）之前再验一次。
    ///
    /// 让点击复用同一个 `verify` 而不是自己再写一遍三项检查：写两遍的那一遍迟早会少一项，
    /// 而少的那一项恰好是"句柄被同一应用的另一个窗口回收"——它在 ROADMAP 94 里就是这么
    /// 被漏掉的。
    pub fn remembered(target: CaptureTarget, handle: isize, pid: Option<u32>) -> Self {
        Self {
            target,
            handle,
            pid,
        }
    }

    /// 批准之后、动手之前，确认那个句柄指的还是同一个窗口。
    ///
    /// 三件事，缺一不可：
    /// - 窗口还活着；
    /// - 还属于同一个应用 —— Win32 在窗口销毁后会把句柄回收，"`IsWindow` 说有效"
    ///   不等于"还是那一个"；
    /// - pid 还是同一个 —— 这是上一条看不出来的那一半：句柄被**同一应用**的新窗口
    ///   回收时应用名照样对得上，而那是最容易发生的一种（Chrome 关一个窗口开一个）。
    ///   批准时取不到 pid 就只能退回到比应用名，这一点在 SECURITY.md 里写明了。
    ///
    /// 标题**允许**变：它会跟着未读数、播放进度、脏标记不停跳，把标题相等当成条件会让
    /// 用户批准过的截图被频繁拒掉，而模型的合理反应是再问一次 —— 那正是审批疲劳。
    /// 代价说清楚：同一个窗口在这段时间里换了内容（切了标签页）时，截到的是新内容，
    /// 所以记录里写的是**截图时**的标题，与批准时不同则两个都写。
    ///
    /// `not_done` 是拒绝话术的尾巴（"nothing was captured" / "nothing was clicked"）：
    /// 同一套身份检查服务两个动作，而告诉用户"因此什么都没发生"时必须说对是哪一个。
    pub fn verify(
        &self,
        current: Option<&DesktopWindow>,
        current_pid: Option<u32>,
        not_done: &str,
    ) -> Result<CaptureTarget, String> {
        let Some(current) = current else {
            return Err(format!("The approved window has closed, so {}.", not_done));
        };
        let resolved = CaptureTarget::from_window(current);
        if crate::services::computer::normalize_app_name(&resolved.app)
            != crate::services::computer::normalize_app_name(&self.target.app)
        {
            return Err(format!(
                "The approved window is gone and its handle now belongs to another app, so {}.",
                not_done
            ));
        }
        if let (Some(approved), Some(now)) = (self.pid, current_pid) {
            if approved != now {
                return Err(format!(
                    "The approved window is gone and its handle now belongs to another window of \
                     the same app, so {}.",
                    not_done
                ));
            }
        }
        Ok(resolved)
    }
}

#[cfg(windows)]
mod platform {
    use super::{
        check_capture_pixels, encode_png, select_capture_target, ApprovedWindow, CaptureTarget,
        WindowCapture,
    };
    use crate::services::computer::DesktopWindow;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetWindowDC,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
        RGBQUAD,
    };
    // `PrintWindow` 在 windows-sys 里挂在 `Storage::Xps` 下（它和 XPS 打印共用一个
    // 头文件区段），不在 `UI::WindowsAndMessaging`。
    use windows_sys::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};

    /// 连被别的窗口挡住的部分也一起渲染。
    ///
    /// 自己定义是因为 windows-sys 0.59 只导出了 `PW_CLIENTONLY`；值取自 Win32 头文件
    /// （`PW_RENDERFULLCONTENT = 0x00000002`）。少了这一位，用 DWM 合成的窗口
    /// （Chrome、Electron、终端）会截出一张全黑的图。
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = 2;

    /// 选出要截的窗口，但**不截**。
    ///
    /// 像素上限在这里就检查：一个反正会被拒的窗口不该先去问人。
    pub fn resolve_capture_target(
        app_filter: Option<&str>,
        title_contains: Option<&str>,
        allowlist: &[String],
    ) -> Result<ApprovedWindow, String> {
        let (target, hwnd) = resolve(app_filter, title_contains, allowlist)?;
        check_capture_pixels(target.width, target.height)?;
        let handle = hwnd as isize;
        Ok(ApprovedWindow {
            target,
            handle,
            pid: crate::services::computer::window_pid(handle),
        })
    }

    /// 截下用户批准的那个窗口。
    ///
    /// 拿句柄值换回 `HWND`，先确认它还活着、还是同一个进程的同一个窗口（见
    /// `ApprovedWindow::verify`），再画。**不**按标题重新找一遍：那会截到另一个同名
    /// 窗口，而标题是被披露方自己就能改的。
    pub fn capture_approved_window(approved: &ApprovedWindow) -> Result<WindowCapture, String> {
        let current = crate::services::computer::describe_window(approved.handle);
        let current_pid = crate::services::computer::window_pid(approved.handle);
        let resolved = approved.verify(current.as_ref(), current_pid, "nothing was captured")?;
        // 尺寸可能在批准之后变了，所以上限要按现在的尺寸重新算
        check_capture_pixels(resolved.width, resolved.height)?;
        let rgba = copy_window_pixels(approved.handle as HWND, resolved.width, resolved.height)?;
        let png = encode_png(resolved.width, resolved.height, &rgba)?;
        Ok(WindowCapture {
            target: resolved,
            png,
        })
    }

    fn resolve(
        app_filter: Option<&str>,
        title_contains: Option<&str>,
        allowlist: &[String],
    ) -> Result<(CaptureTarget, HWND), String> {
        let handles = crate::services::computer::list_windows_with_handles()?;
        let windows: Vec<DesktopWindow> =
            handles.iter().map(|(window, _)| window.clone()).collect();
        let target = select_capture_target(&windows, app_filter, title_contains, allowlist)?;
        let hwnd = handles
            .iter()
            .find(|(window, _)| window == &target)
            .map(|(_, hwnd)| *hwnd)
            .ok_or_else(|| "The window disappeared before it could be captured.".to_string())?;
        Ok((CaptureTarget::from_window(&target), hwnd))
    }

    /// 把窗口内容画进一张离屏位图再读出来，返回 RGBA。
    ///
    /// 用 `PrintWindow(PW_RENDERFULLCONTENT)` 而不是 `BitBlt` 屏幕：后者会把压在上面的
    /// 其他窗口一起抄下来 —— 那既是错的图，也是一次没被授权的披露。
    fn copy_window_pixels(hwnd: HWND, width: u32, height: u32) -> Result<Vec<u8>, String> {
        // SAFETY: 每个句柄都在同一个函数里创建并在所有返回路径上释放；宽高来自
        // `GetWindowRect` 且已经过像素上限检查，所以缓冲区大小不会溢出。
        unsafe {
            let window_dc: HDC = GetWindowDC(hwnd);
            if window_dc.is_null() {
                return Err("GetWindowDC failed for that window.".to_string());
            }
            let memory_dc = CreateCompatibleDC(window_dc);
            if memory_dc.is_null() {
                ReleaseDC(hwnd, window_dc);
                return Err("CreateCompatibleDC failed.".to_string());
            }
            let bitmap = CreateCompatibleBitmap(window_dc, width as i32, height as i32);
            if bitmap.is_null() {
                DeleteDC(memory_dc);
                ReleaseDC(hwnd, window_dc);
                return Err("CreateCompatibleBitmap failed.".to_string());
            }
            let previous = SelectObject(memory_dc, bitmap as _);

            let printed = PrintWindow(hwnd, memory_dc, PW_RENDERFULLCONTENT);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // 负高度 = 自上而下的行序。正数会给出上下翻转的图，而"图是倒的"
                    // 这种错模型不会报告，它只会看错。
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };
            let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
            // 读之前先把位图从 DC 里取出来：`GetDIBits` 的契约要求 `hbmp` 不处于选中状态。
            // 现在的 GDI 实现容忍这一点，但契约就是契约，而"某台机器上截图全黑"是一种
            // 只有那台机器的用户会遇到、且没法从日志里看出来的坏法。
            SelectObject(memory_dc, previous);
            let rows = GetDIBits(
                memory_dc,
                bitmap,
                0,
                height,
                buffer.as_mut_ptr() as *mut _,
                &mut info,
                DIB_RGB_COLORS,
            );

            DeleteObject(bitmap as _);
            DeleteDC(memory_dc);
            ReleaseDC(hwnd, window_dc);

            if printed == 0 {
                return Err(
                    "PrintWindow refused to render that window (it may be protected).".to_string(),
                );
            }
            // 少抄了几行也要报错。缓冲区是零初始化的，所以不会泄露别的内存，但没抄到的
            // 部分会被下面那圈 alpha 补成**不透明的黑**，模型会把它当成真实内容读。
            if rows != height as i32 {
                return Err(format!(
                    "GetDIBits copied {} of {} scan lines, so the capture would be partly blank.",
                    rows, height
                ));
            }
            // GDI 给的是 BGRA，PNG 要 RGBA；同时把 alpha 拍成不透明 —— `PrintWindow`
            // 对很多窗口不写 alpha，照抄会得到一张全透明的图。
            for pixel in buffer.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
                pixel[3] = 0xFF;
            }
            Ok(buffer)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{ApprovedWindow, WindowCapture};

    const UNSUPPORTED: &str = "Window capture is only implemented on Windows.";

    pub fn resolve_capture_target(
        _app_filter: Option<&str>,
        _title_contains: Option<&str>,
        _allowlist: &[String],
    ) -> Result<ApprovedWindow, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn capture_approved_window(_approved: &ApprovedWindow) -> Result<WindowCapture, String> {
        Err(UNSUPPORTED.to_string())
    }
}

pub use platform::{capture_approved_window, resolve_capture_target};

#[cfg(test)]
mod tests {
    use super::*;

    /// 枚举给出的 app 名字是**带扩展名**的（`chrome.exe`），大小写照抄系统。测试里的
    /// 假窗口必须长这样 —— 上一版写成 `chrome`，于是"筛选条件永远不命中"这个缺陷被
    /// 一个不可能出现的值盖住了。
    fn window(title: &str, app: &str) -> DesktopWindow {
        DesktopWindow {
            title: title.to_string(),
            app: app.to_string(),
            bounds: (0, 0, 800, 600),
            foreground: false,
        }
    }

    /// 没有筛选条件时不截图：多窗口桌面上"随便截一个"就是抽奖，而抽错是不可撤回的披露。
    #[test]
    fn capturing_without_a_filter_is_refused() {
        let windows = [window("Inbox", "chrome.exe")];
        let error =
            select_capture_target(&windows, None, None, &["*".to_string()]).expect_err("refused");
        assert!(error.contains("Name the window"), "{}", error);
        // 空白标题等于没给条件
        assert!(select_capture_target(&windows, None, Some("   "), &["*".to_string()]).is_err());
    }

    /// 白名单之外的窗口连"存在"都不该暴露：错误话术只说白名单里有几个。
    #[test]
    fn a_window_outside_the_allow_list_is_not_capturable_or_named() {
        let windows = [window("Signal — Alice", "signal.exe")];
        let error = select_capture_target(&windows, Some("signal"), None, &["chrome".to_string()])
            .expect_err("refused");
        assert!(error.contains("No capturable window matched"), "{}", error);
        assert!(!error.contains("Alice"), "{}", error);
        assert!(!error.contains("signal"), "{}", error);
    }

    /// 命中多个时不猜：列出候选让模型自己缩小。
    #[test]
    fn an_ambiguous_match_captures_nothing_and_lists_the_candidates() {
        let windows = [
            window("Docs — pricing", "chrome.exe"),
            window("Docs — roadmap", "chrome.exe"),
        ];
        let error =
            select_capture_target(&windows, Some("chrome"), Some("docs"), &["*".to_string()])
                .expect_err("refused");
        assert!(error.contains("2 windows matched"), "{}", error);
        assert!(error.contains("pricing"), "{}", error);
        assert!(error.contains("roadmap"), "{}", error);

        let one = select_capture_target(&windows, None, Some("pricing"), &["*".to_string()])
            .expect("one match");
        assert_eq!(one.title, "Docs — pricing");
    }

    /// `app` 这个筛选条件必须真的能命中枚举出来的窗口。
    ///
    /// 这条是本轮审计逼出来的：`matches_target` 原来只归一化模型给的那一侧，拿它去比
    /// 系统给的 `chrome.exe`，于是 `chrome`、`chrome.exe`、整条路径**全都不命中** ——
    /// 一个通告给模型、却永远返回"没找到"的死参数。三种写法都要过。
    #[test]
    fn the_app_filter_matches_the_names_the_enumeration_actually_produces() {
        let windows = [window("Inbox", "chrome.exe")];
        for spelling in ["chrome", "chrome.exe", "Chrome.EXE", "C:\\x\\chrome.exe"] {
            assert!(
                matches_target(&windows[0], Some(spelling), None),
                "spelling {} should match chrome.exe",
                spelling
            );
        }
        assert!(!matches_target(&windows[0], Some("firefox"), None));

        // 而且要能一路走到选中：只测 `matches_target` 会漏掉白名单那一步
        let picked =
            select_capture_target(&windows, Some("chrome.exe"), None, &["chrome".to_string()])
                .expect("the allow list spelling and the filter spelling both normalize");
        assert_eq!(picked.app, "chrome.exe");
    }

    /// 像素上限本身的边界。它在拷贝之前被调用（见 `resolve_capture_target`），这条只钉数值。
    #[test]
    fn the_pixel_limit_refuses_more_than_it_allows() {
        assert!(check_capture_pixels(2560, 1440).is_ok());
        assert!(check_capture_pixels(0, 1080).is_err());
        let error = check_capture_pixels(7680, 4320).expect_err("too many pixels");
        assert!(error.contains("capture limit"), "{}", error);
    }

    /// 批准的是**那一个**窗口，靠句柄认，不靠标题重新找。
    ///
    /// 这条钉的是身份判断本身：窗口关了要拒；句柄被回收给另一个应用要拒；句柄被回收给
    /// **同一应用的另一个窗口**也要拒（只有 pid 看得出来，而这是最容易发生的一种）；
    /// 标题变了不拒（未读数、播放进度、脏标记都会让标题跳，拒掉等于把用户刚批准的
    /// 截图退回去，而模型的合理反应是再问一次 —— 那正是审批疲劳）。
    ///
    /// 92 那一版按 `title == title && app == app` 重新找窗口，于是"批准 A、截到 B"只需要
    /// A 关掉而另一个同名窗口存在 —— 而网页标题就是窗口标题，模型自己就能安排。
    #[test]
    fn the_approved_window_is_identified_by_handle_and_pid_not_by_title() {
        let approved = ApprovedWindow {
            target: CaptureTarget::from_window(&window("Docs — pricing", "chrome.exe")),
            handle: 0x1234,
            pid: Some(4242),
        };

        // 关掉了
        let error = approved
            .verify(None, None, "nothing was captured")
            .expect_err("closed");
        assert!(error.contains("closed"), "{}", error);

        // 句柄被回收给了另一个应用的窗口
        let recycled = window("Docs — pricing", "signal.exe");
        let error = approved
            .verify(Some(&recycled), Some(4242), "nothing was captured")
            .expect_err("another app");
        assert!(error.contains("another app"), "{}", error);

        // 句柄被回收给了同一个应用的另一个窗口：应用名一样、标题一样，只有 pid 不同
        let sibling = window("Docs — pricing", "chrome.exe");
        let error = approved
            .verify(Some(&sibling), Some(99), "nothing was captured")
            .expect_err("same app, another window");
        assert!(
            error.contains("another window of the same app"),
            "{}",
            error
        );

        // 同一个窗口，标题跳了：放行，而且返回的是**现在**的标题，记录才说得对
        let ticked = window("(3) Docs — pricing", "chrome.exe");
        let resolved = approved
            .verify(Some(&ticked), Some(4242), "nothing was captured")
            .expect("same window");
        assert_eq!(resolved.title, "(3) Docs — pricing");

        // 应用名的写法差异不算换应用
        let respelled = window("Docs — pricing", "CHROME.EXE");
        assert!(approved
            .verify(Some(&respelled), Some(4242), "nothing was captured")
            .is_ok());
    }

    /// 批准时读不到 pid 就只能退回到比应用名 —— 这一档降级要明说，不能假装它不存在。
    #[test]
    fn a_target_without_a_pid_falls_back_to_the_app_name() {
        let approved = ApprovedWindow {
            target: CaptureTarget::from_window(&window("Docs — pricing", "chrome.exe")),
            handle: 0x1234,
            pid: None,
        };

        let sibling = window("Docs — pricing", "chrome.exe");
        assert!(approved
            .verify(Some(&sibling), Some(99), "nothing was captured")
            .is_ok());
        let other_app = window("Docs — pricing", "signal.exe");
        assert!(approved
            .verify(Some(&other_app), Some(99), "nothing was captured")
            .is_err());
    }

    /// 批准框里那一行必须说得出是哪个窗口、多大。
    ///
    /// 一句"要截个图吗"等于请用户为看不见的东西签字，而这是本产品最重的一次披露。
    #[test]
    fn the_prompt_line_names_the_window_and_its_size() {
        let described =
            CaptureTarget::from_window(&window("Signal — Alice", "signal.exe")).describe();

        assert!(described.contains("Signal — Alice"), "{}", described);
        assert!(described.contains("signal.exe"), "{}", described);
        assert!(described.contains("800x600"), "{}", described);
    }

    /// 从一帧记下来的身份重建之后，同一套检查要服务点击，而且话术要说对是哪个动作。
    ///
    /// 点击那条路径最容易出的错就是"身份检查只覆盖了截图"：句柄被同一应用的另一个窗口
    /// 回收时，那一下会落在一个谁也没批准过的窗口上。这条测试就是钉住它复用的是同一套。
    #[test]
    fn a_remembered_window_is_verified_for_clicking_too() {
        let approved = ApprovedWindow::remembered(
            CaptureTarget {
                title: "Docs — pricing".to_string(),
                app: "chrome.exe".to_string(),
                width: 800,
                height: 600,
            },
            0x1234,
            Some(4242),
        );

        // 句柄被同一应用的另一个窗口回收：应用名对得上，pid 不同
        let sibling = window("Docs — pricing", "chrome.exe");
        let error = approved
            .verify(Some(&sibling), Some(99), "nothing was clicked")
            .expect_err("same app, another window");
        assert!(
            error.contains("another window of the same app"),
            "{}",
            error
        );
        // 尾巴必须说的是点击，而不是截图 —— 用户看到的是"因此什么都没发生"
        assert!(error.contains("nothing was clicked"), "{}", error);

        // 关掉了同样按点击的话术报
        let closed = approved
            .verify(None, None, "nothing was clicked")
            .expect_err("closed");
        assert!(closed.contains("nothing was clicked"), "{}", closed);

        // 还是同一个窗口就放行，返回的是现在的标题
        assert!(approved
            .verify(Some(&sibling), Some(4242), "nothing was clicked")
            .is_ok());
    }

    /// 真的截一个真窗口。
    ///
    /// GDI 那一段（`PrintWindow` + `GetDIBits`）此前一行覆盖都没有：stride 算错、位图上下
    /// 颠倒、句柄漏掉，纯函数测试全是绿的。这条从"按应用名和标题找到它"一路走到"解出来的
    /// PNG 和它自己报的尺寸一致"。
    ///
    /// 断言落在"这张图描述的就是它说的那个窗口"，不落在像素内容上：这个测试窗口没有背景
    /// 画刷，画出来什么颜色由系统和合成器决定，钉像素只会钉住这台机器。
    #[cfg(windows)]
    #[test]
    fn a_real_window_is_captured_at_the_size_it_reports() {
        let _window = crate::services::computer::test_support::TestWindow::open(
            "Agent IDE capture test",
            420,
            300,
        );
        // 给窗口一点时间画出第一帧，否则 `PrintWindow` 拿到的可能是一张还没内容的位图
        std::thread::sleep(std::time::Duration::from_millis(200));
        let app = std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .expect("测试进程应该有可执行文件名");

        let approved = resolve_capture_target(Some(&app), Some("capture test"), &["*".to_string()])
            .expect("刚开的窗口应该找得到");
        let capture = capture_approved_window(&approved).expect("应该截得出来");

        // 解出来的尺寸必须和它自己报的一致 —— 这两个数字来自两条不同的路径
        let decoder = png::Decoder::new(std::io::Cursor::new(&capture.png));
        let mut reader = decoder.read_info().expect("应该是合法 PNG");
        let mut pixels = vec![0u8; reader.output_buffer_size().expect("known size")];
        let info = reader.next_frame(&mut pixels).expect("一帧");
        assert_eq!(
            (info.width, info.height),
            (capture.target.width, capture.target.height)
        );
        // 每像素 4 字节：少一个通道会让整张图错位，而那在缩略图上看不出来
        assert_eq!(
            info.buffer_size(),
            (capture.target.width as usize) * (capture.target.height as usize) * 4
        );
        assert!(capture.target.title.contains("capture test"));
    }

    /// 编码出来的必须是真的 PNG，而且尺寸不匹配要报错而不是写出一张坏图。
    #[test]
    fn the_encoder_writes_a_real_png_and_refuses_a_mismatched_buffer() {
        let rgba = vec![0xAAu8; 2 * 2 * 4];
        let png = encode_png(2, 2, &rgba).expect("encodes");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // 解回来确认宽高和像素都对得上，而不是只看签名
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = decoder.read_info().expect("valid png");
        let mut out = vec![0u8; reader.output_buffer_size().expect("known size")];
        let info = reader.next_frame(&mut out).expect("one frame");
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&out[..info.buffer_size()], &rgba[..]);

        assert!(encode_png(2, 2, &[0u8; 4]).is_err());
    }
}
