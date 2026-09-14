//! 浏览器控制的 IDE 侧入口。
//!
//! 这一层刻意只服务**用户自己的操作**（命令面板），Agent 还拿不到这些能力：一次导航
//! 无法撤销，而这个产品的前提是"看得见、撤得回"。要交给 Agent，先得有一份和 diff
//! 同等级别的动作记录，以及一条按站点的授权 —— 那是下一步，见 ROADMAP 53。

use crate::services::browser::{self, BrowserTab};

/// 列出可以被驱动的标签页。
#[tauri::command]
pub async fn browser_list_tabs() -> Result<Vec<BrowserTab>, String> {
    browser::list_tabs(browser::configured_port()).await
}

/// 在用户的浏览器里新开一个标签页。
#[tauri::command]
pub async fn browser_open_url(url: String) -> Result<BrowserTab, String> {
    browser::open_url(browser::configured_port(), &url).await
}
