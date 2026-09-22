//! 会话历史落盘：让"新建会话"和"回到之前那次对话"有东西可列。
//!
//! 为什么要有这个文件：对话历史此前只活在 `AgentOrchestrator.conversation` 里，进程一退
//! 就没了。于是界面上"新建会话"无从表达（本来就只有一个）、"历史会话"更无从列出 —— 用户
//! 唯一能做的是把上下文整个清掉，而清掉之后前一件事就永久消失了。
//!
//! 落在配置目录而不是 localStorage：和 `external_log` 同一个理由 —— 这份记录在后端没有第
//! 二份副本，放进 webview 等于让一次"清除站点数据"把它抹掉。`.agent-ide/` 还在 Agent 写入
//! 的 deny-list 上，所以 Agent 自己的写入工具也改不了它。
//!
//! **这里存的是上下文，不是审查状态**。一个会话记的是"模型会看到的那几轮"，不含 steps /
//! diffs：diff 描述的是磁盘某一刻的样子，隔天恢复出来多半已经对不上，而"界面显示的和实际
//! 的不一致"正是这个产品要避免的。所以恢复一个历史会话 = 恢复模型上下文，界面上必须这么说。

use crate::agent::orchestrator::ConversationTurn;
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const STORE_FILE: &str = "sessions.json";

/// 磁盘上保留的会话数上限。
///
/// 跨工作区共用一个文件，所以代价写在这里而不是留给人猜：一个忙碌的工作区会把另一个工作
/// 区的旧会话挤掉。50 是"列表还能一眼看完"和"够回溯几天"之间的折中。
const MAX_SESSIONS: usize = 50;

/// 一个落盘的会话。
///
/// `next_turn_id` 也要存：轮次 id 是"从这一轮切掉上下文"唯一的定位方式，恢复之后如果编号
/// 从 1 重新发，新记的一轮就会和恢复出来的某一轮撞 id —— 用户点第 5 条、切掉的是第 1 条。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSession {
    pub id: String,
    /// 这个会话属于哪个工作区。列表按它过滤 —— 另一个项目的对话在这里只是噪音。
    pub workspace: String,
    /// 会话标题。取第一轮用户提问，和界面上的任务标题同一套规则。
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub next_turn_id: u64,
    pub turns: Vec<ConversationTurn>,
}

fn store_path() -> PathBuf {
    workspace::config_dir().join(STORE_FILE)
}

/// 读整个文件。文件不存在是 `Ok(vec![])`，读不出来是 `Err`。
///
/// 这两件事必须分开：不存在可以直接写，读不出来不能当成"空的"再覆盖 —— 那会把全部历史会话
/// 一次抹掉，而用户下一步很可能正是想回到其中某一个。
fn read_all() -> Result<Vec<StoredSession>, String> {
    let content = match std::fs::read_to_string(store_path()) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("read: {}", error)),
    };
    serde_json::from_str(&content).map_err(|error| format!("parse: {}", error))
}

fn write_all(sessions: &[StoredSession]) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(sessions).map_err(|error| format!("serialize: {}", error))?;
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("create dir: {}", error))?;
    }
    std::fs::write(&path, content).map_err(|error| format!("write: {}", error))
}

/// 当前工作区的会话，最近更新的在前。
///
/// 读不出来只当空：这是一个列表展示，炸掉面板换不回任何信息。真正需要让用户知道的是**写**
/// 失败 —— 那意味着此刻这一轮对话不会被记住，见 `upsert`。
pub fn list_for_current_workspace() -> Vec<StoredSession> {
    let Some(workspace) = workspace::current_workspace_key() else {
        return Vec::new();
    };
    let mut sessions: Vec<StoredSession> = read_all()
        .unwrap_or_default()
        .into_iter()
        .filter(|session| session.workspace == workspace)
        .collect();
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    sessions
}

pub fn find(id: &str) -> Option<StoredSession> {
    read_all()
        .unwrap_or_default()
        .into_iter()
        .find(|session| session.id == id)
}

/// 写入（或覆盖）一个会话。
///
/// 按 id 覆盖而不是追加：同一个会话在它活着的期间会写很多次（每记一轮就写一次），追加会让
/// 文件里出现同一个会话的十几个版本，列表也就重复十几行。
///
/// 超额时按 `updated_at` 砍最旧的，并且**保证刚写的这个留下**：否则在一个已经有 50 条历史
/// 的机器上，新会话可能刚写进去就被自己这次截断砍掉，表现为"新建会话永远不出现在历史里"。
pub fn upsert(session: &StoredSession) -> Result<(), String> {
    let mut sessions = read_all()?;
    match sessions.iter_mut().find(|item| item.id == session.id) {
        Some(existing) => *existing = session.clone(),
        None => sessions.push(session.clone()),
    }
    if sessions.len() > MAX_SESSIONS {
        sessions.sort_by(|a, b| {
            // 刚写的这个排在最前，不参与"最旧"的评比
            let a_current = a.id == session.id;
            let b_current = b.id == session.id;
            b_current
                .cmp(&a_current)
                .then(b.updated_at.cmp(&a.updated_at))
        });
        sessions.truncate(MAX_SESSIONS);
    }
    write_all(&sessions)
}

/// 删掉一个会话。不存在也算成功 —— 用户要的结果（它不在列表里）已经成立。
pub fn remove(id: &str) -> Result<(), String> {
    let mut sessions = read_all()?;
    let before = sessions.len();
    sessions.retain(|session| session.id != id);
    if sessions.len() == before {
        return Ok(());
    }
    write_all(&sessions)
}

/// 毫秒时间戳。会话列表按它排序，也在界面上显示成"多久以前"。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv {
        dir: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TestEnv {
        fn new(workspace: &str) -> Self {
            let guard = workspace::env_test_guard();
            let dir = std::env::temp_dir()
                .join(format!("agent-ide-session-store-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &dir);
            workspace::save_workspace_path(workspace).unwrap();
            Self { dir, _guard: guard }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn session(id: &str, workspace: &str, updated_at: u64) -> StoredSession {
        StoredSession {
            id: id.to_string(),
            workspace: workspace.to_string(),
            title: format!("title {}", id),
            created_at: 1,
            updated_at,
            next_turn_id: 1,
            turns: Vec::new(),
        }
    }

    #[test]
    fn a_session_survives_a_round_trip_and_lists_newest_first() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("old", "C:\\work\\project", 10)).unwrap();
        upsert(&session("new", "C:\\work\\project", 20)).unwrap();

        let listed = list_for_current_workspace();
        assert_eq!(
            listed.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"]
        );
    }

    /// 另一个项目的对话出现在这个项目的历史里只是噪音，而且会让人以为点错了。
    #[test]
    fn sessions_from_another_workspace_are_not_listed() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("mine", "C:\\work\\project", 10)).unwrap();
        upsert(&session("theirs", "C:\\work\\other", 20)).unwrap();

        let listed = list_for_current_workspace();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "mine");
        // 但它仍然在文件里 —— 换回那个工作区还能看到
        assert!(find("theirs").is_some());
    }

    /// 同一个会话每记一轮就写一次，重复写不能变成重复行。
    #[test]
    fn writing_the_same_session_twice_replaces_it() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("s1", "C:\\work\\project", 10)).unwrap();
        let mut updated = session("s1", "C:\\work\\project", 30);
        updated.title = "renamed".to_string();
        upsert(&updated).unwrap();

        let listed = list_for_current_workspace();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "renamed");
    }

    /// 满了之后砍的必须是最旧的那个，而不是刚写进去的这个。
    #[test]
    fn the_session_just_written_survives_the_cap() {
        let _env = TestEnv::new("C:\\work\\project");

        for index in 0..MAX_SESSIONS {
            upsert(&session(
                &format!("s{:02}", index),
                "C:\\work\\project",
                1000 + index as u64,
            ))
            .unwrap();
        }
        // 时间戳刻意比所有已有会话都旧：只按 updated_at 砍的话它会被自己这次截断砍掉
        upsert(&session("fresh", "C:\\work\\project", 1)).unwrap();

        let listed = list_for_current_workspace();
        assert_eq!(listed.len(), MAX_SESSIONS);
        assert!(listed.iter().any(|s| s.id == "fresh"));
        assert!(
            !listed.iter().any(|s| s.id == "s00"),
            "最旧的那个才该被挤掉"
        );
    }

    #[test]
    fn removing_a_session_leaves_the_others_alone() {
        let _env = TestEnv::new("C:\\work\\project");
        upsert(&session("keep", "C:\\work\\project", 10)).unwrap();
        upsert(&session("drop", "C:\\work\\project", 20)).unwrap();

        remove("drop").unwrap();
        // 不存在也算成功：用户要的结果已经成立
        remove("never-existed").unwrap();

        let listed = list_for_current_workspace();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "keep");
    }

    /// 文件坏了不能当成"空的"再覆盖 —— 那会把全部历史一次抹掉。
    #[test]
    fn a_corrupt_store_is_reported_instead_of_overwritten() {
        let env = TestEnv::new("C:\\work\\project");
        std::fs::write(env.dir.join(STORE_FILE), "{ not json").unwrap();

        let error = upsert(&session("s1", "C:\\work\\project", 10)).unwrap_err();

        assert!(error.contains("parse"), "{}", error);
        // 原文还在，人还能自己救
        let raw = std::fs::read_to_string(env.dir.join(STORE_FILE)).unwrap();
        assert_eq!(raw, "{ not json");
    }
}
