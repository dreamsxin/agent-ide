//! 往一个用户批准过的窗口里注入一次鼠标动作：点击、双击、右键，或者滚轮。
//!
//! 这是这个产品里最狠的一个能力，比导航和截图都狠：这些动作撤不回，而且它能点掉任何一
//! 个确认框 —— 包括本产品自己弹出的那个。所以这里的形状和别处不一样：
//!
//! - **坐标只能对着一张模型已经看过的截图给。** 动作必须带上一次 `computer_capture` 留下
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
//!
//! 一个例外要说清楚：**滚轮不是按坐标派送的**。`WM_MOUSEWHEEL` 由系统发给焦点窗口（或者
//! 在"悬停即滚动"打开时发给指针下面那个窗口），所以这里的坐标只决定指针移到哪儿，具体
//! 哪个控件收到那几格是系统定的。这条保证不了，所以写在这里、也写在工具描述和 SECURITY.md
//! 里，而不是让批准框暗示它有点的精度。

/// 往窗口里注入的那一下具体是什么。
///
/// 一个枚举而不是几个布尔参数（`right: bool, double: bool`）：那四种组合里"右键双击"在任何
/// 界面里都没有意义，而用类型把它排除掉比在运行时判断它可靠。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Click,
    DoubleClick,
    RightClick,
    /// 滚轮。正数是向上（远离用户），和 Win32 `WHEEL_DELTA` 同向。
    Scroll {
        notches: i32,
    },
}

impl Gesture {
    /// 给批准框、记录和返回值用的人话。
    ///
    /// 三处共用一句：用户在框里看到的和事后在记录里读到的必须是同一件事，分开写迟早会分叉。
    pub fn describe(&self) -> String {
        match self {
            Gesture::Click => "a left click".to_string(),
            Gesture::DoubleClick => "a double left click".to_string(),
            Gesture::RightClick => "a right click (opens the context menu)".to_string(),
            Gesture::Scroll { notches } if *notches > 0 => {
                format!("a scroll up by {} notch(es)", notches)
            }
            Gesture::Scroll { notches } => {
                format!("a scroll down by {} notch(es)", notches.unsigned_abs())
            }
        }
    }

    /// 记录和批准框里用的动作类型名。
    ///
    /// 和 `describe()` 放在一起，是为了让"用户批准的那句话"和"事后记录里的那个类型"从同一
    /// 个地方长出来：两边分开写的时候，滚动被记成 `computer_click` 这种事没人会发现。
    pub fn record_kind(&self) -> &'static str {
        match self {
            Gesture::Click | Gesture::DoubleClick | Gesture::RightClick => "computer_click",
            Gesture::Scroll { .. } => "computer_scroll",
        }
    }

    /// 解析点击类动作的参数。
    ///
    /// 不认识的值要**拒绝**，不能退回左键：模型想右键而我们点了左键，在一个撤不回的动作上
    /// 是最坏的一种猜。
    pub fn from_click_action(action: Option<&str>) -> Result<Self, String> {
        match action.unwrap_or("click") {
            "click" => Ok(Gesture::Click),
            "double_click" => Ok(Gesture::DoubleClick),
            "right_click" => Ok(Gesture::RightClick),
            other => Err(format!(
                "\"{}\" is not a click action; use \"click\", \"double_click\" or \"right_click\".",
                other
            )),
        }
    }
}

/// 一次滚动能滚多少格。
///
/// 上限 10 格（约 30 行）：更多就该拆成几次调用，因为**每一次**都要人批准 —— 一个参数里
/// 藏着"滚 500 格"等于把一次批准变成了无限授权。0 格拒绝而不是当成成功：什么都没发生却
/// 报成功，模型会以为页面已经到底了。
pub fn check_scroll_notches(notches: i32) -> Result<i32, String> {
    if notches == 0 {
        return Err("A scroll of 0 notches would do nothing.".to_string());
    }
    // `unsigned_abs` 而不是 `abs`：`i32::MIN.abs()` 在 debug 下直接 panic，而这个数是模型
    // 从 JSON 里给进来的，一个越界参数不该把进程带走。
    if notches.unsigned_abs() > 10 {
        return Err(format!(
            "{} notches is more than one approved scroll should move; 10 is the limit, so ask \
             again for the rest.",
            notches
        ));
    }
    Ok(notches)
}

/// 一次动作的坐标是否落在那一帧里面。
///
/// 上界是排他的：宽 800 的图上 x=800 是外面第一列。差一个像素在这里不是小事 ——
/// 落在窗口外面的那一下会打到别的窗口上。
pub fn check_click_inside(width: u32, height: u32, x: u32, y: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("That frame has no pixels, so there is nowhere to aim.".to_string());
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

/// 现在这个窗口还是不是截图时那个尺寸。
///
/// 纯函数，因为"差不多对"在这里等于随机点一下，而这条判断只在窗口被缩放过之后才会触发 ——
/// 那种时候没人在看日志。
pub fn check_same_size(now: (u32, u32), frame: (u32, u32)) -> Result<(), String> {
    if now == frame {
        return Ok(());
    }
    Err(format!(
        "That window is now {}x{} but the frame you captured was {}x{}; capture it again before \
         aiming, because the coordinates no longer point at the same thing.",
        now.0, now.1, frame.0, frame.1
    ))
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
        return Err(
            "The virtual desktop has no size, so the pointer cannot be placed.".to_string(),
        );
    }
    let dx = screen_x - virtual_left;
    let dy = screen_y - virtual_top;
    if dx < 0 || dy < 0 || dx >= virtual_width || dy >= virtual_height {
        return Err(format!(
            "({}, {}) is outside the desktop, so nothing was sent.",
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
    use super::{check_same_size, normalized_absolute, Gesture};
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::WHEEL_DELTA;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
        SetCursorPos, SetForegroundWindow, WindowFromPoint, GA_ROOT, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// 等窗口真的到前台的时长上限，以及查一次的间隔。
    ///
    /// `SetForegroundWindow` 是**异步**的：它返回非零只说明请求被接受了，切换还没完成。
    /// 不等就发的话，那一下会落在此刻仍然盖在上面的那个窗口上 —— 而"必须能置前"这条保证
    /// 的全部意义就是防这件事。500 ms 是个上限而不是固定等待：正常情况下第一次查就通过，
    /// 而一直不通过说明系统拒绝了这次前台切换（Win32 只在调用方拥有前台权限时才允许）。
    const FOREGROUND_ATTEMPTS: u32 = 20;
    const FOREGROUND_POLL: std::time::Duration = std::time::Duration::from_millis(25);

    /// 在用户批准过的那个窗口里的 (x, y) 上做一次 `gesture`。
    ///
    /// 调用方**必须**先确认窗口身份（`ApprovedWindow::verify`）并检查坐标在帧内；这里只
    /// 负责置前、尺寸复核和真正那一下，因为这三件事只有在 Win32 这一侧才做得到。
    ///
    /// 顺序是：先置前 → 确认真的到了前台 → **再**量尺寸。反过来（先量再置前）会漏掉激活
    /// 本身带来的变化 —— 一个从最小化被激活的窗口，尺寸正是在那一刻才变回来的。
    ///
    /// 所有手势走同一条路：置前、遮挡、归一化、指针复位这四件事对右键和滚轮和左键一样
    /// 重要，分成几个函数只会让其中某一条在某一支上被漏掉。
    pub fn send_gesture(
        handle: isize,
        frame_width: u32,
        frame_height: u32,
        x: u32,
        y: u32,
        gesture: Gesture,
    ) -> Result<(), String> {
        let hwnd = handle as HWND;
        // SAFETY: `hwnd` 来自这次运行里刚刚 verify 过的窗口。
        // Win32 只在调用方拥有前台权限时才允许置前，所以这条在真实使用里会遇到。
        if unsafe { SetForegroundWindow(hwnd) } == 0 {
            return Err(
                "Could not bring that window to the front, so nothing was sent — the input would \
                 have landed on whatever is on top of it."
                    .to_string(),
            );
        }
        if !wait_for_foreground(hwnd) {
            return Err(
                "That window did not come to the front in time, so nothing was sent — the input \
                 would have landed on whatever is still on top of it."
                    .to_string(),
            );
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `rect` 是本地变量，`hwnd` 同上。
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return Err("Could not measure that window, so nothing was sent.".to_string());
        }
        check_same_size(
            (
                (rect.right - rect.left).max(0) as u32,
                (rect.bottom - rect.top).max(0) as u32,
            ),
            (frame_width, frame_height),
        )?;
        let point_x = rect.left + x as i32;
        let point_y = rect.top + y as i32;
        // 前台 ≠ 没被挡住。置顶窗口（通知气泡、always-on-top 小工具）可以盖在一个"是前台"
        // 的窗口上面，而 `SendInput` 打的是屏幕坐标 —— 那一下会进盖住它的那个窗口。所以在
        // 发之前问一次"这个点现在属于谁"，这也顺带把置前确认和发送之间那点时间差压到最小。
        // SAFETY: `WindowFromPoint` / `GetAncestor` 只读窗口管理器状态，没有出参。
        let under_point = unsafe {
            WindowFromPoint(POINT {
                x: point_x,
                y: point_y,
            })
        };
        let root_under_point = if under_point.is_null() {
            std::ptr::null_mut()
        } else {
            unsafe { GetAncestor(under_point, GA_ROOT) }
        };
        if root_under_point != hwnd {
            return Err(format!(
                "Something is covering ({}, {}) in that window, so nothing was sent — it would \
                 have gone to whatever is on top. Bring the window fully into view and capture \
                 it again.",
                x, y
            ));
        }

        let (normalized_x, normalized_y) = normalized_absolute(
            point_x,
            point_y,
            // SAFETY: `GetSystemMetrics` 只读系统配置，没有出参。
            unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
        )?;

        // 点完把指针放回去：不放回去的话，指针会停在 Agent 瞄的那个位置上，改变 hover
        // 状态，也改变用户下一次真实点击的起点。
        let mut cursor_before = POINT { x: 0, y: 0 };
        // SAFETY: `cursor_before` 是本地变量。
        let cursor_known = unsafe { GetCursorPos(&mut cursor_before) } != 0;

        // 一个手势的所有事件一次发：分几次 `SendInput` 的话，用户在中间那一刻动一下真鼠标，
        // 按下和抬起就会发生在两个不同的位置 —— 那是一次拖拽，不是点击；双击的两下也会
        // 因为中间插进来的真实移动被系统当成两次单击。
        let mouse = |flags: u32, data: i32, dx: i32, dy: i32| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    // 滚轮的格数走这里；按键事件必须是 0，非 0 在按键事件上是"第几个 X 键"。
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut events: Vec<INPUT> =
            std::iter::once(mouse(
                absolute_flags(MOUSEEVENTF_MOVE),
                0,
                normalized_x,
                normalized_y,
            ))
            .chain(gesture_events(gesture).into_iter().map(|(flags, data)| {
                mouse(absolute_flags(flags), data, normalized_x, normalized_y)
            }))
            .collect();
        // SAFETY: `events` 是本地数组，长度和 `INPUT` 的大小都按 Win32 要求传。
        let sent = unsafe {
            SendInput(
                events.len() as u32,
                events.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != events.len() {
            // 只送出去一部分的时候，已经发出去的按下可能没有配对的抬起 —— 桌面会停在
            // "拖拽中"。补的是**按 `sent` 算出来还按着的那些键**，而不是这个手势定义里按过
            // 的键：批次在第一个事件（移动）就断掉时一个键都没按下，这时候补一个抬起等于凭空
            // 多发一个输入事件，它可能刚好完成用户自己正按着的那一下。补得成不成都照样报失败。
            let mut release: Vec<INPUT> = buttons_still_held(gesture, sent as usize)
                .into_iter()
                .map(|flags| mouse(absolute_flags(flags), 0, normalized_x, normalized_y))
                .collect();
            if !release.is_empty() {
                // SAFETY: 同上。
                unsafe {
                    SendInput(
                        release.len() as u32,
                        release.as_mut_ptr(),
                        std::mem::size_of::<INPUT>() as i32,
                    )
                };
            }
            // 失败这条路上也要把指针放回去：指针停在 Agent 瞄的位置上会改变 hover 状态，
            // 也改变用户下一次真实点击的起点 —— 这一点和成功那条路没有区别。
            restore_cursor(cursor_known, cursor_before);
            return Err(format!(
                "Only {} of {} input events were accepted; {} may be incomplete, and any button \
                 left held down by the accepted part was released.",
                sent,
                events.len(),
                gesture.describe()
            ));
        }
        restore_cursor(cursor_known, cursor_before);
        Ok(())
    }

    /// 把指针放回点之前的位置。
    fn restore_cursor(known: bool, before: POINT) {
        if known {
            // SAFETY: 只写光标位置，参数是本地变量里的坐标。
            unsafe { SetCursorPos(before.x, before.y) };
        }
    }

    /// 绝对坐标事件必须带的两个标志。
    ///
    /// `MOUSEEVENTF_VIRTUALDESK` 是这里最容易漏、漏了最贵的一个：**没有**它的时候，绝对
    /// 坐标按 Win32 文档是映射到**主显示器**，而我们归一化用的是整个虚拟桌面的范围。单屏时
    /// 两者恰好相等，所以漏掉毫无症状；一接上第二块屏，落点就被压缩到主屏上的某个位置 ——
    /// 目标窗口在副屏上被确认成了前台，而那一下点在主屏中央，运行期间那里很可能正是本应用
    /// 自己的批准框。
    ///
    /// 真正决定落点的只有那个移动事件：按键和滚轮事件上的 `dx`/`dy` 不被当成位置（没有
    /// `MOUSEEVENTF_MOVE`就没有移动）。它们照样带上这两个标志是为了整批一致 —— 让"绝对
    /// 坐标怎么解释"只有一个答案，而不是每个事件各自一份。
    fn absolute_flags(base: u32) -> u32 {
        base | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK
    }

    /// 一个手势展开成的 `(事件标志, mouseData)` 序列，移动那一下不算在内。
    ///
    /// 抽成纯函数是为了能断言它：模型要右键而我们发了左键，或者双击只发出去一下，在真实
    /// 注入里只能靠人盯着屏幕才发现，而那时动作已经做了。
    fn gesture_events(gesture: Gesture) -> Vec<(u32, i32)> {
        match gesture {
            Gesture::Click => vec![(MOUSEEVENTF_LEFTDOWN, 0), (MOUSEEVENTF_LEFTUP, 0)],
            // 双击就是两次按下抬起。Win32 没有"双击事件"——是窗口自己按系统的双击间隔
            // 把两下并起来的，所以这四个必须在同一个批次里，中间不能插进真实输入。
            Gesture::DoubleClick => vec![
                (MOUSEEVENTF_LEFTDOWN, 0),
                (MOUSEEVENTF_LEFTUP, 0),
                (MOUSEEVENTF_LEFTDOWN, 0),
                (MOUSEEVENTF_LEFTUP, 0),
            ],
            Gesture::RightClick => vec![(MOUSEEVENTF_RIGHTDOWN, 0), (MOUSEEVENTF_RIGHTUP, 0)],
            // 一格是 `WHEEL_DELTA`。直接传格数的话滚动量会小到几乎看不出来，模型会重试。
            Gesture::Scroll { notches } => {
                vec![(MOUSEEVENTF_WHEEL, notches * WHEEL_DELTA as i32)]
            }
        }
    }

    /// 发送只完成了 `sent` 个事件时，还按着哪些键 —— 要补的抬起。
    ///
    /// 按 `sent` 算而不是按手势定义算：批次在第一个事件（移动）就断掉时一个键都没按下，
    /// 这时候补一个抬起是一个凭空多出来的输入事件，它可能刚好完成用户自己正按着的那一下。
    /// `sent` 里含最前面那个移动事件，所以按键序列要跳过它。
    fn buttons_still_held(gesture: Gesture, sent: usize) -> Vec<u32> {
        let mut held: Vec<u32> = Vec::new();
        for (flags, _) in gesture_events(gesture)
            .into_iter()
            .take(sent.saturating_sub(1))
        {
            match flags {
                MOUSEEVENTF_LEFTDOWN => held.push(MOUSEEVENTF_LEFTUP),
                MOUSEEVENTF_RIGHTDOWN => held.push(MOUSEEVENTF_RIGHTUP),
                MOUSEEVENTF_LEFTUP => held.retain(|flag| *flag != MOUSEEVENTF_LEFTUP),
                MOUSEEVENTF_RIGHTUP => held.retain(|flag| *flag != MOUSEEVENTF_RIGHTUP),
                // 滚轮不按任何键
                _ => {}
            }
        }
        held
    }

    /// 轮询到那个窗口真的成为前台，或者等够了。
    fn wait_for_foreground(hwnd: HWND) -> bool {
        for _ in 0..FOREGROUND_ATTEMPTS {
            // SAFETY: `GetForegroundWindow` 没有参数也没有出参。
            if unsafe { GetForegroundWindow() } == hwnd {
                return true;
            }
            std::thread::sleep(FOREGROUND_POLL);
        }
        false
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 绝对坐标必须按**整个虚拟桌面**解释。
        ///
        /// 漏掉 `MOUSEEVENTF_VIRTUALDESK` 在单屏上完全没有症状（虚拟桌面就等于主屏），
        /// 一接上第二块屏，落点就被压缩到主屏上的某个位置 —— 目标窗口在副屏上被确认成了
        /// 前台，而那一下点在主屏中央，运行期间那里很可能正是本应用自己的批准框。这条
        /// 测试就是为了让那个漏掉在单屏机器上也能被发现。
        #[test]
        fn absolute_events_are_mapped_to_the_whole_virtual_desktop() {
            let flags = absolute_flags(MOUSEEVENTF_LEFTDOWN);

            assert_ne!(flags & MOUSEEVENTF_VIRTUALDESK, 0, "0x{:x}", flags);
            assert_ne!(flags & MOUSEEVENTF_ABSOLUTE, 0, "0x{:x}", flags);
            // 基础事件不能被覆盖掉
            assert_ne!(flags & MOUSEEVENTF_LEFTDOWN, 0, "0x{:x}", flags);
        }

        /// 每个手势发的键必须是它自己那一套。
        ///
        /// 右键发成左键、双击只发一下，都是"做了一件别的、撤不回的事"；而这条链上唯一
        /// 会发现它的人是事后看屏幕的用户。
        #[test]
        fn each_gesture_sends_its_own_buttons() {
            assert_eq!(
                gesture_events(Gesture::Click),
                vec![(MOUSEEVENTF_LEFTDOWN, 0), (MOUSEEVENTF_LEFTUP, 0)]
            );
            assert_eq!(
                gesture_events(Gesture::RightClick),
                vec![(MOUSEEVENTF_RIGHTDOWN, 0), (MOUSEEVENTF_RIGHTUP, 0)]
            );
            // 双击是两下，不是一下
            assert_eq!(gesture_events(Gesture::DoubleClick).len(), 4);
            assert!(gesture_events(Gesture::DoubleClick)
                .iter()
                .all(|(flags, _)| *flags == MOUSEEVENTF_LEFTDOWN || *flags == MOUSEEVENTF_LEFTUP));
            // 右键绝不能出现在左键手势里
            for gesture in [Gesture::Click, Gesture::DoubleClick] {
                assert!(gesture_events(gesture)
                    .iter()
                    .all(|(flags, _)| *flags != MOUSEEVENTF_RIGHTDOWN));
            }
        }

        /// 滚轮要按 `WHEEL_DELTA` 换算，方向不能反。
        ///
        /// 直接把格数填进 `mouseData` 的话滚动量是 1/120，几乎看不出动，模型会反复重试；
        /// 方向反了则是往相反的方向翻页。
        #[test]
        fn scrolling_is_measured_in_wheel_deltas() {
            assert_eq!(
                gesture_events(Gesture::Scroll { notches: 3 }),
                vec![(MOUSEEVENTF_WHEEL, 3 * WHEEL_DELTA as i32)]
            );
            let (_, down) = gesture_events(Gesture::Scroll { notches: -2 })[0];
            assert!(down < 0, "{}", down);
        }

        /// 只发出去一半时补的抬起，只能是**那一半真的按下了、又还没抬起**的键。
        ///
        /// 按手势定义算（"点击按过左键，所以补左键"）在批次断在第一个事件上时是错的：那时
        /// 一个键都没按下，补出来的抬起是一个凭空多出来的输入事件，它可能刚好完成用户自己
        /// 正按着的那一下 —— 也就是把一次失败变成了一次没人批准的点击。
        #[test]
        fn only_the_buttons_left_held_by_the_accepted_part_are_released() {
            // 什么都没发出去，或者只发出去那个移动：一个键都没按下
            assert!(buttons_still_held(Gesture::Click, 0).is_empty());
            assert!(buttons_still_held(Gesture::Click, 1).is_empty());
            // 移动 + 按下：左键正按着
            assert_eq!(
                buttons_still_held(Gesture::Click, 2),
                vec![MOUSEEVENTF_LEFTUP]
            );
            // 整批都出去了：已经配对抬起过了
            assert!(buttons_still_held(Gesture::Click, 3).is_empty());
            assert_eq!(
                buttons_still_held(Gesture::RightClick, 2),
                vec![MOUSEEVENTF_RIGHTUP]
            );
            // 双击断在第二下的按下上
            assert_eq!(
                buttons_still_held(Gesture::DoubleClick, 4),
                vec![MOUSEEVENTF_LEFTUP]
            );
            assert!(buttons_still_held(Gesture::DoubleClick, 3).is_empty());
            // 滚轮任何位置都不按键
            for sent in 0..=2 {
                assert!(buttons_still_held(Gesture::Scroll { notches: 1 }, sent).is_empty());
            }
        }
    }
}

#[cfg(windows)]
pub use platform::send_gesture;

/// 非 Windows 上没有实现。
///
/// 这个桩不会被调用到 —— `allows_input()` 里带着 `cfg!(windows)`，别的平台上点击工具
/// 连通告都没有。留着它是为了让这个模块在所有平台上都能编译，而不是靠 `cfg` 把整块
/// 代码从视野里藏起来。
#[cfg(not(windows))]
pub fn send_gesture(
    _handle: isize,
    _frame_width: u32,
    _frame_height: u32,
    _x: u32,
    _y: u32,
    _gesture: Gesture,
) -> Result<(), String> {
    Err("Sending mouse input to a desktop window is only implemented on Windows.".to_string())
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

    /// 尺寸不一致就拒，而且要说出两个尺寸 —— 模型的下一步是重新截图，它需要知道差在哪。
    #[test]
    fn a_resized_window_is_refused_and_both_sizes_are_named() {
        assert!(check_same_size((800, 600), (800, 600)).is_ok());

        let error = check_same_size((1024, 600), (800, 600)).unwrap_err();
        assert!(error.contains("1024x600"), "{}", error);
        assert!(error.contains("800x600"), "{}", error);
        assert!(error.contains("capture it again"), "{}", error);
    }

    /// 不认识的动作名要拒绝，不能退回左键。
    ///
    /// 退回左键意味着模型想打开右键菜单而我们真的点了一下那个位置上的东西 —— 在一个撤不回
    /// 的动作上，猜错比拒绝贵得多。
    #[test]
    fn an_unknown_click_action_is_refused_instead_of_falling_back() {
        assert_eq!(Gesture::from_click_action(None).unwrap(), Gesture::Click);
        assert_eq!(
            Gesture::from_click_action(Some("click")).unwrap(),
            Gesture::Click
        );
        assert_eq!(
            Gesture::from_click_action(Some("double_click")).unwrap(),
            Gesture::DoubleClick
        );
        assert_eq!(
            Gesture::from_click_action(Some("right_click")).unwrap(),
            Gesture::RightClick
        );

        let error = Gesture::from_click_action(Some("middle_click")).unwrap_err();
        assert!(error.contains("middle_click"), "{}", error);
        // 错误里要列出可用的名字，否则模型只能继续猜
        assert!(error.contains("double_click"), "{}", error);
    }

    /// 批准框里那句话和记录里那句话是同一句，而且认得出方向。
    #[test]
    fn every_gesture_describes_itself_in_words_a_person_can_approve() {
        assert!(Gesture::RightClick.describe().contains("right"));
        assert!(Gesture::DoubleClick.describe().contains("double"));
        assert!(Gesture::Scroll { notches: 3 }.describe().contains("up"));
        let down = Gesture::Scroll { notches: -3 }.describe();
        assert!(down.contains("down"), "{}", down);
        // 方向是词而不是负号：批准框里"-3"读不出是往哪边
        assert!(!down.contains("-3"), "{}", down);
    }

    /// 一次批准只能换一次有界的滚动。
    ///
    /// 0 格报成功会让模型以为已经滚到底；上限则是因为**每一次调用**才要人批准一次，
    /// 一个参数里藏着"滚 500 格"等于把一次批准变成了无限授权。
    #[test]
    fn a_scroll_is_bounded_and_a_zero_scroll_is_refused() {
        assert_eq!(check_scroll_notches(1).unwrap(), 1);
        assert_eq!(check_scroll_notches(-10).unwrap(), -10);
        assert!(check_scroll_notches(0).is_err());
        assert!(check_scroll_notches(11).is_err());
        assert!(check_scroll_notches(-11).is_err());
        assert!(check_scroll_notches(i32::MIN).is_err());
    }
}

/// 真的造一个窗口，真的点它，看它有没有收到。
///
/// 这是 `SendInput` 那一行唯一的覆盖 —— **但只在这个会话肯把前台交出来的时候**。
/// `SetForegroundWindow` 只在调用方当前拥有前台权限时才成功，所以从一个不在前台的终端里
/// 跑 `cargo test`（以及不少 CI 会话）会走到"拒绝"那一支。两支都断言，但只有一支碰得到
/// `SendInput`，测试会把走的是哪一支打出来 —— 覆盖率的真相要看得见，不能靠一句"有 e2e"。
///
/// - 允许置前时：断言那个窗口确实收到了一次左键按下，落点在它的客户区里；
/// - 拒绝置前时：断言 `send_gesture` **拒绝**了而不是硬发，且一下都没发出去。
#[cfg(all(test, windows))]
mod injection_tests {
    use super::{send_gesture, Gesture};
    use crate::services::computer::test_support::TestWindow;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

    #[test]
    fn a_click_reaches_the_window_it_was_aimed_at() {
        let window = TestWindow::open("Agent IDE click test", 480, 360);

        // 点客户区中间偏上的一个点。坐标是**窗口矩形**相对的，和 `CaptureFrame` 一致。
        let (x, y) = (200_u32, 180_u32);
        let outcome = send_gesture(window.handle, 480, 360, x, y, Gesture::Click);
        // 留点时间让那一下走完消息队列
        std::thread::sleep(std::time::Duration::from_millis(300));

        match outcome {
            Ok(()) => {
                println!("injection path exercised: the window was clicked for real");
                assert_eq!(
                    TestWindow::clicks(),
                    1,
                    "置前成功却没收到点击：说明那一下落在了别的地方"
                );
                // 落点要在客户区里。断言的是"落进了这个窗口"，不是某个精确像素 ——
                // 窗口边框和标题栏的厚度是系统主题决定的，钉死它只会钉住这台机器。
                let mut client = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                // SAFETY: `client` 是本地变量；句柄来自上面那个还活着的窗口。
                unsafe { GetClientRect(window.handle as _, &mut client) };
                let (px, py) = TestWindow::last_click();
                assert!(
                    px >= 0 && px < client.right && py >= 0 && py < client.bottom,
                    "落点 ({}, {}) 不在客户区 {}x{} 里",
                    px,
                    py,
                    client.right,
                    client.bottom
                );
                // 窗口矩形相对的 x 和客户区 x 只差左边框，所以横向不该偏太多
                assert!(
                    (px - x as i32).abs() <= 32,
                    "横向偏了 {} 像素，坐标映射错了",
                    (px - x as i32).abs()
                );
            }
            Err(error) => {
                println!("injection path not exercised here: {}", error);
                // 这台机器/这个会话不让置前：那就必须是**拒绝**，而不是硬发出去。
                // 认的是"置前"这一类拒绝，不是任意错误 —— 尺寸不符、被遮挡都有自己的话术，
                // 混在一起会让这条测试对任何失败都点头。
                assert!(
                    error.contains("front"),
                    "置前失败时唯一可接受的结果是拒绝，实际是：{}",
                    error
                );
                assert_eq!(
                    TestWindow::clicks(),
                    0,
                    "既然拒绝了，就不该有任何一下被发出去"
                );
            }
        }
    }
}
