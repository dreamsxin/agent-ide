//! Agent 层往前端发事件的出口。
//!
//! orchestrator 以前直接持有 `AppHandle`，于是它的每个方法都只能在跑着的桌面
//! 应用里调用 —— 包括那些除了发一条通知之外纯粹在改状态的方法。测试拿不到
//! `AppHandle`（把 Tauri runtime 拉进 lib 测试二进制会让整个套件在加载阶段就
//! 起不来），所以 orchestrator 里的流水线逻辑一直没有任何自动化验证。
//!
//! 这个 trait 只做一件事：把"发事件"变成一个可替换的依赖。桌面端传
//! `AppHandle`，测试传 `RecordingEvents` 并对发出的事件做断言，没有前端在听的
//! headless 入口传 `SilentEvents`。
//!
//! 载荷统一成 `serde_json::Value` 而不是泛型 `impl Serialize`：trait object 需要
//! 对象安全，而调用方本来大多已经在 `serde_json::to_value(...)` 了。

use serde_json::Value;

pub trait RunEvents: Send + Sync {
    fn emit_json(&self, event: &str, payload: Value);
}

impl RunEvents for tauri::AppHandle {
    fn emit_json(&self, event: &str, payload: Value) {
        use tauri::Emitter;
        // 发送失败（窗口已关闭之类）不该打断运行，和改造前的 `let _ =` 一致
        let _ = self.emit(event, payload);
    }
}

/// 不发事件。给没有前端在听的入口用。
pub struct SilentEvents;

impl RunEvents for SilentEvents {
    fn emit_json(&self, _event: &str, _payload: Value) {}
}

/// 记录发出过哪些事件，供测试断言。
///
/// 断言事件而不只断言返回值是有意义的：前端的状态完全靠这些事件驱动，
/// 一个"逻辑正确但没发事件"的运行在界面上等于什么都没发生。
#[derive(Default)]
pub struct RecordingEvents {
    emitted: std::sync::Mutex<Vec<(String, Value)>>,
}

impl RecordingEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按发出顺序返回事件名
    pub fn names(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>()
    }

    pub fn payloads_for(&self, event: &str) -> Vec<Value> {
        self.entries()
            .into_iter()
            .filter(|(name, _)| name == event)
            .map(|(_, payload)| payload)
            .collect()
    }

    pub fn count(&self, event: &str) -> usize {
        self.payloads_for(event).len()
    }

    fn entries(&self) -> Vec<(String, Value)> {
        // 锁中毒时返回已记录的内容而不是 panic：测试断言失败的信息应该来自
        // 断言本身，而不是来自记录器
        self.emitted
            .lock()
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }
}

impl RunEvents for RecordingEvents {
    fn emit_json(&self, event: &str, payload: Value) {
        if let Ok(mut entries) = self.emitted.lock() {
            entries.push((event.to_string(), payload));
        }
    }
}
