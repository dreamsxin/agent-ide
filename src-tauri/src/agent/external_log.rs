//! 把外部动作记录写到磁盘，让它活过一次重启。
//!
//! 为什么必须落盘：外部动作（导航、窗口截图、读页面正文）是这个产品里**撤不回**的那
//! 一类，而这份记录是它唯一的补偿。此前它只活在 orchestrator 的内存里 —— 关掉应用，
//! "它做过什么"就再没有人知道了，`ExternalActionRecord.run_id` 那句"重启后前端拿它和
//! 恢复出来的会话对账"也就无从谈起。
//!
//! 为什么不是 localStorage：`diffs` 在后端有一份权威副本，localStorage 那份只是显示用
//! 的镜像；外部动作没有第二份。把唯一那份放进 webview，等于让任何前端代码、和一次
//! "清除站点数据"都能把审计记录抹掉。配置目录（`.agent-ide`）还在 Agent 写入的
//! deny-list 上，所以连 Agent 自己的写入工具也改不了这个文件。

use crate::agent::orchestrator::ExternalActionRecord;
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const LOG_FILE: &str = "external-actions.json";

/// 磁盘上保留的条数上限。
///
/// 比内存里的 200 大：那 200 是**一次会话**的量，而这一个文件跨会话、也跨工作区共用。
/// 代价写在这里而不是留给人猜：一个忙碌的工作区会把另一个工作区的旧记录挤掉。
const MAX_PERSISTED_ACTIONS: usize = 500;

/// 一条落盘的记录：审计记录本身，加上它属于哪个工作区。
///
/// 直接内嵌 `ExternalActionRecord` 而不是另写一个磁盘结构：那会变成第二份必须和它保持
/// 同步的定义，而"两份状态"正是这个仓库反复修掉的那类缺陷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedAction {
    pub workspace: String,
    #[serde(flatten)]
    pub action: ExternalActionRecord,
}

fn log_path() -> PathBuf {
    workspace::config_dir().join(LOG_FILE)
}

/// 记录属于哪个工作区，读和写都从这里取。
///
/// 一个函数而不是两处各自去问：读的键和写的键只要有一处不一样，记录就会写进去却读不
/// 回来 —— 那时界面看起来和"根本没落盘"完全一样。
fn current_workspace() -> Option<String> {
    workspace::load_workspace_path()
        .ok()
        .flatten()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// 读整个文件。读不出来或解析不了就当空。
///
/// 不返回错误：一份坏掉的审计文件不该让应用起不来，而在界面上"这里没有记录"和"文件坏
/// 了"能给用户的下一步是同一个。真正要防的是**静默写坏**，那靠下面的追加语义 ——
/// 解析失败时不覆盖，见 `append_for_current_workspace`。
fn read_all() -> Result<Vec<PersistedAction>, ()> {
    let content = match std::fs::read_to_string(log_path()) {
        Ok(content) => content,
        // 文件还不存在是正常的第一次运行，和"读坏了"要分开：前者可以写，后者不能覆盖
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(()),
    };
    serde_json::from_str(&content).map_err(|_| ())
}

/// 当前工作区在**之前的会话**里留下的记录，最旧在前 —— 和内存里那份顺序一致。
///
/// 每一条都标上 `restored`：用户看到的是一件撤不回的事，而"这是刚刚发生的"和"这是上
/// 周那次留下的"对他的意义完全不同。界面据此打标，不靠他自己去算时间戳。
pub fn load_for_current_workspace() -> Vec<ExternalActionRecord> {
    let Some(workspace) = current_workspace() else {
        return Vec::new();
    };
    read_all()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.workspace == workspace)
        .map(|entry| ExternalActionRecord {
            restored: true,
            ..entry.action
        })
        .collect()
}

/// 把**这一次新登记的**记录追加到文件里。
///
/// 只收新记录，不收 orchestrator 上那整份列表：传整份的话，每次发布都会把已经落盘的
/// 记录再写一遍，文件每轮翻倍。调用点拿到的正是 `record_external_actions` 的返回值。
pub fn append_for_current_workspace(newly_recorded: &[ExternalActionRecord]) {
    if newly_recorded.is_empty() {
        return;
    }
    let Some(workspace) = current_workspace() else {
        // 没有保存过工作区就不落盘：这条记录没有可靠的归属，写进去之后在任何工作区
        // 下都读不回来，只会是一份谁也看不见的垃圾。
        return;
    };
    // 解析失败**不覆盖**：宁可这一批记录只留在内存里，也不能用一份新文件把之前所有
    // 撤不回动作的记录抹掉 —— 那正是这份文件存在的意义的反面。
    let Ok(existing) = read_all() else {
        return;
    };
    let merged = merge(existing, &workspace, newly_recorded, MAX_PERSISTED_ACTIONS);
    let Ok(content) = serde_json::to_string_pretty(&merged) else {
        return;
    };
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, content);
}

/// 追加并压回上限，丢**最旧**的。
///
/// 纯函数，因为上限这件事只在写满之后才出问题，而那种时候没人在看磁盘。
fn merge(
    mut existing: Vec<PersistedAction>,
    workspace: &str,
    newly_recorded: &[ExternalActionRecord],
    limit: usize,
) -> Vec<PersistedAction> {
    existing.extend(newly_recorded.iter().map(|action| PersistedAction {
        workspace: workspace.to_string(),
        action: ExternalActionRecord {
            // 写进去的时候它不是"恢复出来的"；这个标是读的时候才成立的事实
            restored: false,
            ..action.clone()
        },
    }));
    if existing.len() > limit {
        let excess = existing.len() - limit;
        existing.drain(..excess);
    }
    existing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, kind: &str) -> ExternalActionRecord {
        ExternalActionRecord {
            id: id.to_string(),
            timestamp: "2026-09-21T10:00:00Z".to_string(),
            kind: kind.to_string(),
            target: "https://example.com".to_string(),
            detail: "detail".to_string(),
            run_id: Some("run-1".to_string()),
            restored: false,
        }
    }

    /// 一个独立的配置目录 + 一个保存好的工作区路径。
    ///
    /// 自己设 `AGENT_IDE_CONFIG_DIR`：`env_test_guard()` 只是一把互斥锁，它不设这个变量。
    struct LogEnv {
        dir: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl LogEnv {
        fn new(workspace: &str) -> Self {
            let guard = workspace::env_test_guard();
            let dir = std::env::temp_dir()
                .join(format!("agent-ide-external-log-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("建测试配置目录");
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &dir);
            workspace::save_workspace_path(workspace).expect("保存工作区");
            Self { dir, _guard: guard }
        }
    }

    impl Drop for LogEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 记录必须活过重启，而且要能被认出来是上一次会话留下的。
    #[test]
    fn records_survive_a_restart_and_say_they_did() {
        let _env = LogEnv::new("D:/work/project");

        append_for_current_workspace(&[record("a", "browser_open")]);
        let restored = load_for_current_workspace();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, "a");
        assert_eq!(restored[0].kind, "browser_open");
        // 用户看到的是一件撤不回的事，"刚刚"和"上一次会话"对他不是同一件事
        assert!(restored[0].restored);
    }

    /// 另一个工作区的记录不在这里显示。
    ///
    /// 混在一起比不显示更糟：用户没法分辨哪条属于手上这个项目，而记录的全部用处就是
    /// 复盘"在这里发生过什么"。
    #[test]
    fn another_workspaces_actions_are_not_shown_here() {
        let env = LogEnv::new("D:/work/project-a");
        append_for_current_workspace(&[record("a", "browser_open")]);

        workspace::save_workspace_path("D:/work/project-b").expect("切工作区");
        append_for_current_workspace(&[record("b", "computer_capture")]);

        let in_b = load_for_current_workspace();
        assert_eq!(in_b.len(), 1);
        assert_eq!(in_b[0].id, "b");

        // 切回去，A 的那条还在 —— 换工作区不该抹掉别处的记录
        workspace::save_workspace_path("D:/work/project-a").expect("切回工作区");
        let in_a = load_for_current_workspace();
        assert_eq!(in_a.len(), 1);
        assert_eq!(in_a[0].id, "a");
        drop(env);
    }

    /// 写满了丢最旧的：留下的必须是最近那些，因为最近的才是用户在追的那件事。
    #[test]
    fn a_full_log_drops_the_oldest_not_the_newest() {
        let existing = vec![
            PersistedAction {
                workspace: "w".to_string(),
                action: record("old", "browser_open"),
            },
            PersistedAction {
                workspace: "w".to_string(),
                action: record("mid", "browser_open"),
            },
        ];

        let merged = merge(existing, "w", &[record("new", "browser_open")], 2);

        let ids: Vec<&str> = merged
            .iter()
            .map(|entry| entry.action.id.as_str())
            .collect();
        assert_eq!(ids, vec!["mid", "new"]);
    }

    /// 坏掉的文件读成空，但**不覆盖**它。
    ///
    /// 覆盖是这里最贵的一种错：一份读不出来的文件也许还能人工看，而写一份新的会把之前
    /// 所有撤不回动作的记录一起抹掉。
    #[test]
    fn a_corrupt_log_is_read_as_empty_and_never_overwritten() {
        let _env = LogEnv::new("D:/work/project");
        std::fs::write(log_path(), "not json at all").expect("写坏文件");

        assert!(load_for_current_workspace().is_empty());

        append_for_current_workspace(&[record("a", "browser_open")]);
        assert_eq!(
            std::fs::read_to_string(log_path()).unwrap(),
            "not json at all"
        );
    }

    /// 老版本写的文件（没有 `runId` / `restored`）仍然读得回来。
    ///
    /// 整份 parse 失败就等于把历史记录全丢掉，而这两个字段都是后来才加的。
    #[test]
    fn a_log_written_by_an_older_build_still_loads() {
        let _env = LogEnv::new("D:/work/project");
        std::fs::write(
            log_path(),
            r#"[{"workspace":"D:/work/project","id":"a","timestamp":"2026-09-01T00:00:00Z",
                 "kind":"browser_open","target":"https://example.com","detail":"d"}]"#,
        )
        .expect("写老格式");

        let restored = load_for_current_workspace();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].run_id, None);
        assert!(restored[0].restored);
    }

    /// 没保存过工作区就不写：那条记录没有归属，写进去也读不回来。
    #[test]
    fn without_a_saved_workspace_nothing_is_written() {
        let guard = workspace::env_test_guard();
        let dir =
            std::env::temp_dir().join(format!("agent-ide-external-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("建测试配置目录");
        std::env::set_var("AGENT_IDE_CONFIG_DIR", &dir);

        append_for_current_workspace(&[record("a", "browser_open")]);

        assert!(!log_path().exists());
        std::env::remove_var("AGENT_IDE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
        drop(guard);
    }
}
