//! 往一个用户批准过的窗口里注入一次鼠标点击。
//!
//! 这是这个产品里最狠的一个能力，比导航和截图都狠：一次点击撤不回，而且它能点掉任何一
//! 个确认框 —— 包括本产品自己弹出的那个。所以这里的形状和别处不一样：
//!
//! - **坐标只能对着一张模型已经看过的截图给。** 点击必须带上一次 `computer_capture` 留下
//!   的 frame id；窗口是那一帧决定的，不是筛选条件决定的。"按标题再找一遍"这条路在
//!   ROADMAP 93 里已经被证明是错的（标题是被操作方自己能改的东西），而对点击来说错一次
//!   的代价是点在另一个窗口的另一个位置上。
//! - **窗口尺寸变了就拒绝。** 截图之后窗口被缩放过，图上那个按钮已经不在那个坐标了；
//!   "差不多对"在这里等于随机点一下。
//! - **必须能置前。** 置不前说明有别的窗口盖在上面，而 `SendInput` 打的是屏幕坐标 ——
//!   那一下会落在盖住它的那个窗口上。
//!
//! 为什么是 `SendInput` 而不是给窗口发 `WM_LBUTTONDOWN`：消息能绕过遮挡、不用置前，看起来
//! 更"干净"，但现代 UI 框架（Chrome、Electron、WPF）大量依赖真实的输入队列状态（hover、
//! capture、IME），发消息经常什么都不发生，而"点了但没反应"在这条链上是最坏的结果：
//! 模型会重试。

/// 一次点击的坐标是否落在那一帧里面。
///
/// 上界是排他的：宽 800 的图上 x=800 是外面第一列。差一个像素在这里不是小事 ——
/// 落在窗口外面的那一下会打到别的窗口上。
pub fn check_click_inside(width: u32, height: u32, x: u32, y: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("That frame has no pixels, so there is nowhere to click.".to_string());
    }
    if x >= width || y >= height {
        return Err(format!(
            "({}, {}) is outside the {}x{} frame you captured; coordinates are pixels in that \
             image, measured from its top-left corner.",
            x, y, width, height
        ));
    }
    Ok(())
}

/// 把图上的坐标换成 `SendInput` 要的"归一化绝对坐标"。
///
/// `MOUSEEVENTF_ABSOLUTE` 的坐标不是像素，而是 0..=65535 映射到**整个虚拟桌面**。多屏、
/// 副屏在主屏左边（虚拟桌面原点是负数）时，少减那个原点就会点到另一块屏幕上。
///
/// 除法用的是 `width - 1`：65535 要对应最后一个像素，不是"最后一个像素之后"。按 `width`
/// 除会让最右一列永远点不到，而那一列上常常正好是关闭按钮。
pub fn normalized_absolute(
    screen_x: i32,
    screen_y: i32,
    virtual_left: i32,
    virtual_top: i32,
    virtual_width: i32,
    virtual_height: i32,
) -> Result<(i32, i32), String> {
    if virtual_width <= 1 || virtual_height <= 1 {
        return Err("The virtual desktop has no size, so the click cannot be placed.".to_string());
    }
    let dx = screen_x - virtual_left;
    let dy = screen_y - virtual_top;
    if dx < 0 || dy < 0 || dx >= virtual_width || dy >= virtual_height {
        return Err(format!(
            "({}, {}) is outside the desktop, so nothing was clicked.",
            screen_x, screen_y
        ));
    }
    Ok((
        (dx * 65_535) / (virtual_width - 1),
        (dy * 65_535) / (virtual_height - 1),
    ))
}

#[cfg(windows)]
mod platform {
    use super::normalized_absolute;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowRect, SetForegroundWindow, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// 点一下用户批准过的那个窗口里的 (x, y)。
    ///
    /// 调用方**必须**先确认窗口身份（`ApprovedWindow::verify`）并检查坐标在帧内；这里只
    /// 负责"尺寸还对不对"和真正那一下，因为尺寸只有在 Win32 这一侧才拿得到。
    pub fn click_in_window(
        handle: isize,
        frame_width: u32,
        frame_height: u32,
        x: u32,
        y: u32,
    ) -> Result<(), String> {
        let hwnd = handle as HWND;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `hwnd` 来自这次运行里刚刚 verify 过的窗口；`rect` 是本地变量。
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return Err("Could not measure that window, so nothing was clicked.".to_string());
        }
        let now = (
            (rect.right - rect.left).max(0) as u32,
            (rect.bottom - rect.top).max(0) as u32,
        );
        if now != (frame_width, frame_height) {
            return Err(format!(
                "That window is now {}x{} but the frame you captured was {}x{}; capture it again \
                 before clicking, because the coordinates no longer point at the same thing.",
                now.0, now.1, frame_width, frame_height
            ));
        }
        // 置前失败就拒绝：`SendInput` 打的是屏幕坐标，被盖住时那一下会落在上面那个窗口上。
        // Win32 只在调用方拥有前台权限时才允许置前，所以这条在真实使用里会遇到。
        // SAFETY: 同上。
        if unsafe { SetForegroundWindow(hwnd) } == 0 {
            return Err(
                "Could not bring that window to the front, so the click was not sent — it would \
                 have landed on whatever is on top of it."
                    .to_string(),
            );
        }
        let (normalized_x, normalized_y) = normalized_absolute(
            rect.left + x as i32,
            rect.top + y as i32,
            // SAFETY: `GetSystemMetrics` 只读系统配置，没有出参。
            unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
        )?;

        // 三个事件一次发：移动、按下、抬起。分三次 `SendInput` 的话，用户在中间那一刻
        // 动一下真鼠标，按下和抬起就会发生在两个不同的位置 —— 那是一次拖拽，不是点击。
        let mouse = |flags: u32, dx: i32, dy: i32| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut events = [
            mouse(
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                normalized_x,
                normalized_y,
            ),
            mouse(
                MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE,
                normalized_x,
                normalized_y,
            ),
            mouse(
                MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE,
                normalized_x,
                normalized_y,
            ),
        ];
        // SAFETY: `events` 是本地数组，长度和 `INPUT` 的大小都按 Win32 要求传。
        let sent = unsafe {
            SendInput(
                events.len() as u32,
                events.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != events.len() {
            // 部分送达也算失败，但要说清：按下了没抬起会把桌面留在按住鼠标的状态
            return Err(format!(
                "Only {} of {} input events were accepted; the click may be incomplete.",
                sent,
                events.len()
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use platform::click_in_window;

/// 非 Windows 上没有实现。
///
/// 这个桩不会被调用到 —— `allows_input()` 里带着 `cfg!(windows)`，别的平台上点击工具
/// 连通告都没有。留着它是为了让这个模块在所有平台上都能编译，而不是靠 `cfg` 把整块
/// 代码从视野里藏起来。
#[cfg(not(windows))]
pub fn click_in_window(
    _handle: isize,
    _frame_width: u32,
    _frame_height: u32,
    _x: u32,
    _y: u32,
) -> Result<(), String> {
    Err("Clicking a desktop window is only implemented on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 边界是排他的：宽 800 的图上 x=800 已经在窗口外面，那一下会打到别的窗口上。
    #[test]
    fn a_click_must_land_inside_the_frame_it_was_measured_on() {
        assert!(check_click_inside(800, 600, 0, 0).is_ok());
        assert!(check_click_inside(800, 600, 799, 599).is_ok());
        assert!(check_click_inside(800, 600, 800, 599).is_err());
        assert!(check_click_inside(800, 600, 799, 600).is_err());
        assert!(check_click_inside(0, 600, 0, 0).is_err());
    }

    /// 归一化要把最后一个像素映到 65535，而不是映到"再往右一点"。
    ///
    /// 按 `width` 而不是 `width - 1` 除时，最右一列永远点不到 —— 而那一列上常常是
    /// 关闭按钮。
    #[test]
    fn the_last_pixel_maps_to_the_last_absolute_coordinate() {
        assert_eq!(normalized_absolute(0, 0, 0, 0, 1920, 1080).unwrap(), (0, 0));
        assert_eq!(
            normalized_absolute(1919, 1079, 0, 0, 1920, 1080).unwrap(),
            (65_535, 65_535)
        );
    }

    /// 副屏在主屏左边时虚拟桌面原点是负数，少减它就会点到另一块屏幕上。
    #[test]
    fn a_negative_desktop_origin_is_subtracted() {
        // 虚拟桌面从 -1920 开始，宽 3840：主屏左上角在中点
        let (x, _) = normalized_absolute(0, 0, -1920, 0, 3840, 1080).unwrap();
        assert!((32_000..33_000).contains(&x), "{}", x);
    }

    /// 桌面外的坐标要拒绝，而不是夹到边上。
    ///
    /// 夹一下会让一次"算错了的点击"变成一次"落在角上的点击"，而屏幕角上通常有东西。
    #[test]
    fn a_point_outside_the_desktop_is_refused_not_clamped() {
        assert!(normalized_absolute(-1, 0, 0, 0, 1920, 1080).is_err());
        assert!(normalized_absolute(1920, 0, 0, 0, 1920, 1080).is_err());
        assert!(normalized_absolute(0, 0, 0, 0, 1, 1).is_err());
    }
}
