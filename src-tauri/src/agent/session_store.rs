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
//!
//! 已知代价（和 `external-actions.json` 同一类）：整个文件读-改-写，没有跨进程锁。两个应用
//! 实例共用同一个 `~/.agent-ide` 时后写的那个会覆盖掉前一个的会话。写入走"临时文件 +
//! rename"，所以崩在写入中间不会留下半截文件，但并发覆盖仍然是最后写入者赢。

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
    // 先写临时文件再 rename：直接覆盖的话，崩在写入中间会留下半截 JSON —— 那之后整份历史
    // 都读不出来了，而这个文件的全部意义就是"回得去"。
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, content).map_err(|error| format!("write: {}", error))?;
    std::fs::rename(&temp, &path).map_err(|error| format!("replace: {}", error))
}

/// 把读不出来的那份文件挪到一边，让记录能继续写下去。
///
/// 不是"放弃这次写入"：放弃的后果是一次崩在写入中间留下的半截文件让此后**每一次**落盘都
/// 失败，而用户在界面上只看到一句同样的报错。挪走既保住了那份人还能看的历史，也让新的会话
/// 继续留得下来。和 `external_log` 同一套处理。
fn quarantine_unreadable_store(reason: &str) -> Result<String, String> {
    let path = store_path();
    let moved = path.with_extension(format!("unreadable-{}.json", now_ms()));
    std::fs::rename(&path, &moved)
        .map_err(|error| format!("{}; and it could not be moved aside: {}", reason, error))?;
    Ok(moved.to_string_lossy().to_string())
}

/// 列表加上"这次读/写出了什么问题"。
///
/// 读失败不能只当空列表：那时界面显示的"还没有历史会话"和真的没有历史长得一模一样，而磁盘上
/// 那几十条就在那里。所以把话一起带出去，由面板显示。
#[derive(Debug, Clone, Default)]
pub struct SessionList {
    pub sessions: Vec<StoredSession>,
    pub warning: Option<String>,
}

/// 当前工作区的会话，最近更新的在前。
pub fn list_for_current_workspace() -> SessionList {
    let Some(workspace) = workspace::current_workspace_key() else {
        return SessionList {
            sessions: Vec::new(),
            warning: Some(
                "No workspace has been opened yet, so sessions are not being saved.".to_string(),
            ),
        };
    };
    let (all, warning) = match read_all() {
        Ok(all) => (all, None),
        Err(reason) => (
            Vec::new(),
            Some(format!(
                "The session history file could not be read ({}). Earlier sessions are still in \
                 that file; the next save will move it aside and start a new one.",
                reason
            )),
        ),
    };
    let mut sessions: Vec<StoredSession> = all
        .into_iter()
        .filter(|session| session.workspace == workspace)
        .collect();
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    SessionList { sessions, warning }
}

/// 找一个会话。文件读不出来和"没有这个会话"是两件事，所以不合并成 `Option`：前者报给用户的
/// 话应该是"读不出来"，而不是"它已经不在磁盘上了" —— 后者会让人以为是自己删过。
pub fn find(id: &str) -> Result<Option<StoredSession>, String> {
    Ok(read_all()?.into_iter().find(|session| session.id == id))
}

/// 写入（或覆盖）一个会话。
///
/// 按 id 覆盖而不是追加：同一个会话在它活着的期间会写很多次（每记一轮就写一次），追加会让
/// 文件里出现同一个会话的十几个版本，列表也就重复十几行。
///
/// 超额时按 `updated_at` 砍最旧的，并且**保证刚写的这个留下**：否则在一个已经有 50 条历史
/// 的机器上，新会话可能刚写进去就被自己这次截断砍掉，表现为"新建会话永远不出现在历史里"。
pub fn upsert(session: &StoredSession) -> Result<(), String> {
    let mut sessions = match read_all() {
        Ok(sessions) => sessions,
        Err(reason) => {
            let moved = quarantine_unreadable_store(&reason)?;
            // 这一句要让用户看见：他的旧历史换了个文件名，而这件事在界面上没有别的痕迹
            let warning = format!(
                "The session history file could not be read and was moved to {}. A new one was \
                 started, so this session is being saved.",
                moved
            );
            let mut fresh = vec![session.clone()];
            fresh.truncate(MAX_SESSIONS);
            write_all(&fresh)?;
            return Err(warning);
        }
    };
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

    /// 只看这个测试自己写进去的那些 id。
    ///
    /// `AGENT_IDE_CONFIG_DIR` 是进程级的，而 `push_turn` 现在会落盘：另一个并发跑着的
    /// orchestrator 测试记一轮对话时，`config_dir()` 读到的就是**这个**测试的目录。按 id
    /// 过滤之后，那种串扰不会把断言变成偶发失败。
    fn mine(prefix: &str) -> Vec<String> {
        list_for_current_workspace()
            .sessions
            .into_iter()
            .filter(|session| session.id.starts_with(prefix))
            .map(|session| session.id)
            .collect()
    }

    #[test]
    fn a_session_survives_a_round_trip_and_lists_newest_first() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("rt-old", "C:\\work\\project", 10)).unwrap();
        upsert(&session("rt-new", "C:\\work\\project", 20)).unwrap();

        assert_eq!(mine("rt-"), vec!["rt-new", "rt-old"]);
    }

    /// 另一个项目的对话出现在这个项目的历史里只是噪音，而且会让人以为点错了。
    #[test]
    fn sessions_from_another_workspace_are_not_listed() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("ws-mine", "C:\\work\\project", 10)).unwrap();
        upsert(&session("ws-theirs", "C:\\work\\other", 20)).unwrap();

        assert_eq!(mine("ws-"), vec!["ws-mine"]);
        // 但它仍然在文件里 —— 换回那个工作区还能看到
        assert!(find("ws-theirs").unwrap().is_some());
    }

    /// 同一个会话每记一轮就写一次，重复写不能变成重复行。
    #[test]
    fn writing_the_same_session_twice_replaces_it() {
        let _env = TestEnv::new("C:\\work\\project");

        upsert(&session("dup-s1", "C:\\work\\project", 10)).unwrap();
        let mut updated = session("dup-s1", "C:\\work\\project", 30);
        updated.title = "renamed".to_string();
        upsert(&updated).unwrap();

        assert_eq!(mine("dup-"), vec!["dup-s1"]);
        assert_eq!(find("dup-s1").unwrap().unwrap().title, "renamed");
    }

    /// 满了之后砍的必须是最旧的那个，而不是刚写进去的这个。
    #[test]
    fn the_session_just_written_survives_the_cap() {
        let _env = TestEnv::new("C:\\work\\project");

        for index in 0..MAX_SESSIONS {
            upsert(&session(
                &format!("cap-{:02}", index),
                "C:\\work\\project",
                1000 + index as u64,
            ))
            .unwrap();
        }
        // 时间戳刻意比所有已有会话都旧：只按 updated_at 砍的话它会被自己这次截断砍掉
        upsert(&session("cap-fresh", "C:\\work\\project", 1)).unwrap();

        let listed = mine("cap-");
        assert!(listed.len() <= MAX_SESSIONS);
        assert!(listed.iter().any(|id| id == "cap-fresh"));
        assert!(
            !listed.iter().any(|id| id == "cap-00"),
            "最旧的那个才该被挤掉：{:?}",
            listed
        );
    }

    #[test]
    fn removing_a_session_leaves_the_others_alone() {
        let _env = TestEnv::new("C:\\work\\project");
        upsert(&session("rm-keep", "C:\\work\\project", 10)).unwrap();
        upsert(&session("rm-drop", "C:\\work\\project", 20)).unwrap();

        remove("rm-drop").unwrap();
        // 不存在也算成功：用户要的结果已经成立
        remove("rm-never-existed").unwrap();

        assert_eq!(mine("rm-"), vec!["rm-keep"]);
    }

    /// 文件坏了不能当成"空的"直接覆盖，也不能从此**永久**写不进去。
    ///
    /// 放弃这次写入是最初的做法，后果是：一次崩在写入中间留下的半截文件让之后每一轮对话都
    /// 静默存不下来。所以挪走再重开一份，并且把"你的旧历史现在在这个文件里"当成警告返回。
    #[test]
    fn an_unreadable_store_is_moved_aside_so_recording_continues() {
        let env = TestEnv::new("C:\\work\\project");
        std::fs::write(env.dir.join(STORE_FILE), "{ not json").unwrap();

        let warning = upsert(&session("q-s1", "C:\\work\\project", 10)).unwrap_err();
        assert!(warning.contains("moved to"), "{}", warning);

        // 新的这条真的写进去了，而且之后的写入也不再失败
        assert_eq!(mine("q-"), vec!["q-s1"]);
        upsert(&session("q-s2", "C:\\work\\project", 20)).unwrap();

        // 原文没有被抹掉，人还能自己救
        let quarantined: Vec<String> = std::fs::read_dir(&env.dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("unreadable"))
            .collect();
        assert_eq!(quarantined.len(), 1, "{:?}", quarantined);
        let raw = std::fs::read_to_string(env.dir.join(&quarantined[0])).unwrap();
        assert_eq!(raw, "{ not json");
    }

    /// 读不出来的时候列表不能只是"空的"：那和真的没有历史长得一模一样，而磁盘上还有几十条。
    #[test]
    fn an_unreadable_store_says_so_instead_of_looking_empty() {
        let env = TestEnv::new("C:\\work\\project");
        std::fs::write(env.dir.join(STORE_FILE), "{ not json").unwrap();

        let listed = list_for_current_workspace();

        assert!(listed.sessions.is_empty());
        assert!(
            listed
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("could not be read")),
            "{:?}",
            listed.warning
        );
    }
}
