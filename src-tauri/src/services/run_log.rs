//! 把运行记录落到磁盘。
//!
//! 界面里那个日志面板只活在内存里：窗口一关就没了，用户也没法把它贴给别人。
//! 而排查"点了按钮，只得到一句没用的话"这类问题，唯一的依据就是当时那条记录。
//! `npm run tauri -- dev` 的控制台输出只在那台机器的那个终端里，翻不回去，
//! 也传不出去。所以每条 action log 在发给界面的同时，追加一份到配置目录。
//!
//! **只记有诊断价值的事件。** `agent-diff-ready` / `agent-stream-token` 这类载荷里
//! 是整份文件内容或整段模型输出，写进日志等于把工作区和回复抄一遍 —— 文件几分钟
//! 就能涨到几百兆，而里面没有一句能帮上忙的话。

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;

/// 单个日志文件的上限，超了就转存成 `.1`，只留一份旧的。
///
/// 不按天切分：这份日志是给"刚才那次为什么失败"用的，不是审计留档。留两份足够
/// 覆盖"我重启了一次应用再复现"这种最常见的排查方式。
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// 一条记录里 `details` 的字符上限。模型的完整回复可以有几十 KB，而诊断需要的
/// 一般是开头那几行（错误原话在最前面）。按字符截断，不按字节 —— 中文一个字三字节，
/// 按字节切会把最后一个字切成乱码。
const MAX_DETAILS: usize = 2000;

pub fn log_dir() -> PathBuf {
    crate::services::workspace::config_dir().join("logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join("agent-ide.log")
}

fn rotated_path() -> PathBuf {
    log_dir().join("agent-ide.log.1")
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{}… [+{} chars]", kept, text.chars().count() - limit)
}

/// 这个事件该往磁盘写什么；`None` = 不写。
///
/// 抽成纯函数是为了能测："什么被记下来"和"什么被刻意丢掉"都是这个功能的正确性，
/// 而它们在真实运行里发生在一个不方便断言的地方。
pub fn line_for(event: &str, payload: &Value) -> Option<String> {
    match event {
        "agent-action-log" => {
            let level = payload
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let phase = payload.get("phase").and_then(Value::as_str).unwrap_or("-");
            let summary = payload.get("summary").and_then(Value::as_str).unwrap_or("");
            let details = payload.get("details").and_then(Value::as_str).unwrap_or("");
            let stage = payload.get("stage").and_then(Value::as_str);
            let timestamp = payload
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            let mut line = format!("{} [{}] {}", timestamp, level, phase);
            if let Some(stage) = stage {
                line.push_str(&format!(" ({})", stage));
            }
            line.push_str(&format!(" {}", summary));
            if !details.is_empty() {
                line.push_str(&format!("\n    {}", truncate_chars(details, MAX_DETAILS)));
            }
            Some(line)
        }
        // 状态只记一个词。它是"那一刻界面显示的是什么"的唯一依据，而载荷很小。
        "agent-state-changed" => {
            let state = payload.get("state").and_then(Value::as_str).unwrap_or("?");
            Some(format!(
                "{} [state] {}",
                chrono::Utc::now().to_rfc3339(),
                state
            ))
        }
        _ => None,
    }
}

/// 写一行。失败一律忽略：日志写不进去不该让一次运行中断 —— 用户要的是那次改动，
/// 不是那次改动的记录。
pub fn record(event: &str, payload: &Value) {
    let Some(line) = line_for(event, payload) else {
        return;
    };
    let path = log_path();
    if std::fs::create_dir_all(log_dir()).is_err() {
        return;
    }
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) >= MAX_BYTES {
        let _ = std::fs::rename(&path, rotated_path());
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace;
    use serde_json::json;

    struct ConfigEnv {
        dir: PathBuf,
    }

    impl ConfigEnv {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("agent-ide-log-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("config dir");
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &dir);
            Self { dir }
        }
    }

    impl Drop for ConfigEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn an_action_log_keeps_level_phase_and_reason() {
        let line = line_for(
            "agent-action-log",
            &json!({
                "timestamp": "2026-09-24T10:00:00Z",
                "level": "error",
                "phase": "review_apply",
                "stage": "Implement",
                "summary": "Apply failed",
                "details": "Refusing to overwrite existing file: hello.txt",
            }),
        )
        .expect("recorded");

        assert!(line.contains("2026-09-24T10:00:00Z"));
        assert!(line.contains("[error]"));
        assert!(line.contains("review_apply"));
        assert!(line.contains("(Implement)"));
        // 原话是这份日志存在的全部理由
        assert!(line.contains("Refusing to overwrite existing file: hello.txt"));
    }

    /// 模型的完整回复能有几十 KB。截断按字符算，末尾要留一句"还有多少"。
    #[test]
    fn long_details_are_truncated_with_a_count() {
        let details = "字".repeat(MAX_DETAILS + 50);
        let line = line_for(
            "agent-action-log",
            &json!({ "level": "info", "phase": "stage_done", "summary": "ok", "details": details }),
        )
        .expect("recorded");

        assert!(line.contains("[+50 chars]"));
        // 不能把最后一个汉字切成两半
        assert!(line.chars().filter(|ch| *ch == '\u{fffd}').count() == 0);
    }

    /// 这些事件的载荷是整份文件内容 / 整段回复，记下来只会让日志失去可读性。
    #[test]
    fn bulky_events_are_not_recorded() {
        assert!(line_for("agent-diff-ready", &json!([{ "file": "a.ts" }])).is_none());
        assert!(line_for("agent-stream-token", &json!("token")).is_none());
        assert!(line_for("agent-step-update", &json!({ "id": "1" })).is_none());
    }

    #[test]
    fn state_changes_are_recorded_as_one_word() {
        let line =
            line_for("agent-state-changed", &json!({ "state": "waiting_user" })).expect("recorded");
        assert!(line.contains("[state] waiting_user"));
    }

    #[test]
    fn the_file_rotates_once_it_grows_past_the_limit() {
        let _guard = workspace::env_test_guard();
        let _env = ConfigEnv::new();

        std::fs::create_dir_all(log_dir()).expect("log dir");
        std::fs::write(log_path(), "x".repeat(MAX_BYTES as usize + 1)).expect("seed");

        record(
            "agent-action-log",
            &json!({ "level": "info", "phase": "p", "summary": "after rotation", "details": "" }),
        );

        let current = std::fs::read_to_string(log_path()).expect("current log");
        assert!(current.contains("after rotation"));
        assert!(current.len() < MAX_BYTES as usize, "new file starts fresh");
        assert!(rotated_path().exists(), "the old file is kept as .1");
    }
}
