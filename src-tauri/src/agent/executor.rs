use crate::services::llm_client::{ChatMessage, LlmClient};
use tokio::sync::mpsc;

/// 执行单个步骤：调用 LLM 生成代码变更
pub async fn execute_step(
    llm: &LlmClient,
    step: &str,
    context: &str,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are a coding assistant. Output ONLY the code or diff for the requested step. Be precise and minimal.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Execute this step: {}\n\nContext:\n{}\n\nProvide the implementation.",
                step, context
            ),
        },
    ];

    llm.stream_chat(messages, tx).await
}

/// 从 LLM 响应中解析 diff 块
/// 格式: ```diff: path/to/file\n<<<<<<< ORIGINAL\n...\n=======\n...\n>>>>>>> UPDATED ```
pub fn parse_diffs(response: &str) -> Vec<crate::agent::state_machine::FileDiff> {
    let mut diffs = Vec::new();
    let mut current_file = String::new();
    let mut current_lines = Vec::new();
    let mut in_diff = false;

    for line in response.lines() {
        if line.trim().starts_with("```diff:") || line.trim().starts_with("```diff") {
            in_diff = true;
            // 提取文件名
            if let Some(rest) = line.trim().strip_prefix("```diff:") {
                current_file = rest.trim().to_string();
            } else if let Some(rest) = line.trim().strip_prefix("```diff") {
                current_file = rest.trim().to_string();
            }
            current_lines.clear();
            continue;
        }

        if in_diff && line.trim() == "```" {
            // 结束 diff 块
            if !current_file.is_empty() && !current_lines.is_empty() {
                let content = current_lines.join("\n");
                let old_count = content.lines().filter(|l| l.starts_with('-')).count() as u32;
                let new_count = content.lines().filter(|l| l.starts_with('+')).count() as u32;

                diffs.push(crate::agent::state_machine::FileDiff {
                    id: uuid::Uuid::new_v4().to_string(),
                    file: std::mem::take(&mut current_file),
                    hunks: vec![crate::agent::state_machine::DiffHunk {
                        old_start: 0,
                        old_lines: old_count,
                        new_start: 0,
                        new_lines: new_count,
                        content,
                    }],
                    status: "pending".to_string(),
                });
            }
            in_diff = false;
            continue;
        }

        if in_diff {
            current_lines.push(line.to_string());
        }
    }

    diffs
}
