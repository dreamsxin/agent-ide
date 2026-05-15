use crate::agent::multi_agent::AgentRole;
use crate::services::llm_client::{ChatMessage, LlmClient};
use serde::Deserialize;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::mpsc;

/// 执行步骤的系统提示词
const EXECUTOR_PROMPT: &str = r#"You are a precise coding assistant. Your task is to implement ONE specific coding step.

## Output Format
Provide the implementation for this step. For code changes, you MUST use this diff format:

```diff:path/to/file
<<<<<<< ORIGINAL
existing code to replace
=======
new replacement code
>>>>>>> UPDATED
```

For new files, use:

```new:path/to/file
file content here
```

## Rules
1. Output ONLY code and diffs — no explanations unless no code change is needed
2. Each diff block must have exactly one ORIGINAL and one UPDATED section
3. For edits: show EXACT original code that needs to be replaced
4. Be precise — copy the original code exactly as it appears

Respond now with the implementation."#;

/// 执行单个步骤：调用 LLM 生成代码变更
pub async fn execute_step(
    llm: &LlmClient,
    step: &str,
    context: &str,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: EXECUTOR_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Step to execute: {}\n\nContext:\n{}\n\nProvide the implementation (code/diff only):",
                step, context
            ),
        },
    ];

    llm.stream_chat(messages, cancel_flag, tx).await
}

pub async fn execute_stage(
    llm: &LlmClient,
    role: AgentRole,
    stage_name: &str,
    user_prompt: &str,
    context: &str,
    prior_outputs: &str,
    pending_diffs: &str,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    let output_rules = match role {
        AgentRole::Architect => {
            "Output a concise implementation plan. Do not output code diffs."
        }
        AgentRole::Coder | AgentRole::Tester => {
            r#"When code changes are needed, prefer this structured Agent IDE protocol:

```agent-changes
{
  "changes": [
    {
      "type": "edit",
      "file": "path/to/file",
      "baseHash": "optional current file hash when known",
      "rationale": "why this change is needed",
      "hunks": [
        { "original": "exact existing code", "updated": "replacement code" }
      ]
    },
    {
      "type": "create",
      "file": "path/to/new-file",
      "rationale": "why this file is needed",
      "content": "complete file content"
    }
  ]
}
```

If you cannot produce valid JSON, use Agent IDE diff/new-file blocks. Use explanations only when no code change is needed."#
        }
        AgentRole::Reviewer => {
            r#"Review the actual pending diffs, not just prior text. Use this structure:

## Review Summary
Short verdict.

## Findings
- [severity] file/path: concrete issue or "No blocking findings".

## Verification
- What should be tested or was implicitly checked.

If a blocking fix is required, include an Agent IDE diff/new-file block after the findings."#
        }
    };

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: format!("{}\n\n{}", role.system_prompt(), output_rules),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Pipeline stage: {}\nRole: {}\n\nUser task:\n{}\n\nProject context:\n{}\n\nPrior stage outputs:\n{}\n\nActual pending diffs for review:\n{}\n\nRun this stage now.",
                stage_name,
                role.to_string(),
                user_prompt,
                context,
                if prior_outputs.trim().is_empty() {
                    "(none)"
                } else {
                    prior_outputs
                },
                if pending_diffs.trim().is_empty() {
                    "No pending diffs."
                } else {
                    pending_diffs
                },
            ),
        },
    ];

    llm.stream_chat(messages, cancel_flag, tx).await
}

/// 从 LLM 响应中解析 diff 块
pub fn parse_diffs(response: &str) -> Vec<crate::agent::state_machine::FileDiff> {
    let mut diffs = Vec::new();
    let lines: Vec<&str> = response.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // 检测代码块开始: ```diff:file, ```new:file, ```lang:file
        if let Some((block_type, file)) = detect_block_start(trimmed) {
            let mut block_lines: Vec<String> = Vec::new();
            i += 1;

            // 收集块内容直到 ```
            while i < lines.len() && lines[i].trim() != "```" {
                block_lines.push(lines[i].to_string());
                i += 1;
            }

            match block_type.as_str() {
                "agent-changes" => {
                    let content = block_lines.join("\n");
                    diffs.extend(parse_agent_changes(&content));
                }
                "diff" => {
                    let (original, updated) = split_diff_content(&block_lines);
                    let content = block_lines.join("\n");
                    if !content.trim().is_empty() {
                        diffs.push(make_diff(&file, &content, &original, &updated));
                    }
                }
                "new" | "code" => {
                    let content = block_lines.join("\n");
                    if !content.trim().is_empty() {
                        diffs.push(make_new_file_diff(&file, &content));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    diffs
}

/// 检测代码块类型和文件名: 返回 (类型, 文件名)
fn detect_block_start(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("```")?;
    if rest.is_empty() { return None; }

    // ```diff:file
    if let Some(file) = rest.strip_prefix("diff:") {
        return Some(("diff".into(), file.trim().to_string()));
    }
    if rest == "diff" {
        return Some(("diff".into(), String::new()));
    }

    if rest == "agent-changes" || rest == "agent_changes" || rest == "json:agent-changes" {
        return Some(("agent-changes".into(), String::new()));
    }

    // ```new:file
    if let Some(file) = rest.strip_prefix("new:") {
        return Some(("new".into(), file.trim().to_string()));
    }

    // ```lang:file (e.g. ```typescript:src/app.ts)
    if let Some(idx) = rest.find(':') {
        let file = rest[idx + 1..].trim();
        if !file.is_empty() && file.contains('.') {
            return Some(("code".into(), file.to_string()));
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct AgentChangesBlock {
    changes: Vec<AgentChange>,
}

#[derive(Debug, Deserialize)]
struct AgentChange {
    #[serde(rename = "type")]
    change_type: String,
    file: String,
    #[serde(rename = "baseHash")]
    base_hash: Option<String>,
    rationale: Option<String>,
    content: Option<String>,
    hunks: Option<Vec<AgentChangeHunk>>,
}

#[derive(Debug, Deserialize)]
struct AgentChangeHunk {
    original: String,
    updated: String,
}

fn parse_agent_changes(json: &str) -> Vec<crate::agent::state_machine::FileDiff> {
    let Ok(block) = serde_json::from_str::<AgentChangesBlock>(json) else {
        return Vec::new();
    };

    let mut diffs = Vec::new();
    for change in block.changes {
        match change.change_type.as_str() {
            "create" | "new" => {
                if let Some(content) = change.content {
                    if !change.file.trim().is_empty() && !content.trim().is_empty() {
                        let mut diff = make_new_file_diff(&change.file, &content);
                        if let Some(rationale) = change.rationale {
                            if let Some(hunk) = diff.hunks.first_mut() {
                                hunk.content = format!("rationale: {}\n\n{}", rationale, hunk.content);
                            }
                        }
                        diffs.push(diff);
                    }
                }
            }
            "edit" | "modify" => {
                let Some(hunks) = change.hunks else {
                    continue;
                };
                let parsed_hunks: Vec<_> = hunks
                    .into_iter()
                    .filter(|hunk| !hunk.original.trim().is_empty())
                    .map(|hunk| {
                        let old_count = hunk
                            .original
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                            .max(1) as u32;
                        let new_count = hunk
                            .updated
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                            .max(1) as u32;
                        crate::agent::state_machine::DiffHunk {
                            old_start: 0,
                            old_lines: old_count,
                            new_start: 0,
                            new_lines: new_count,
                            content: change.rationale.clone().unwrap_or_default(),
                            original: hunk.original,
                            updated: hunk.updated,
                        }
                    })
                    .collect();

                if !change.file.trim().is_empty() && !parsed_hunks.is_empty() {
                    diffs.push(crate::agent::state_machine::FileDiff {
                        id: uuid::Uuid::new_v4().to_string(),
                        file: change.file,
                        base_hash: change.base_hash,
                        hunks: parsed_hunks,
                        status: "pending".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    diffs
}

/// 分割 diff 内容为 ORIGINAL 和 UPDATED 两部分
fn split_diff_content(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut original = Vec::new();
    let mut updated = Vec::new();
    let mut in_original = false;
    let mut in_updated = false;

    for line in lines {
        let t = line.trim();
        if t.starts_with("<<<<<<<") { in_original = true; in_updated = false; continue; }
        if t.starts_with("=======") { in_original = false; in_updated = true; continue; }
        if t.starts_with(">>>>>>>") { in_original = false; in_updated = false; continue; }
        if in_original { original.push(line.clone()); }
        else if in_updated { updated.push(line.clone()); }
    }

    (original, updated)
}

fn make_diff(file: &str, content: &str, original: &[String], updated: &[String]) -> crate::agent::state_machine::FileDiff {
    let old_count = original.iter().filter(|l| !l.trim().is_empty()).count().max(1) as u32;
    let new_count = updated.iter().filter(|l| !l.trim().is_empty()).count().max(1) as u32;

    crate::agent::state_machine::FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: old_count,
            new_start: 0,
            new_lines: new_count,
            content: content.to_string(),
            original: original.join("\n"),
            updated: updated.join("\n"),
        }],
        status: "pending".to_string(),
    }
}

fn make_new_file_diff(file: &str, content: &str) -> crate::agent::state_machine::FileDiff {
    let count = content.lines().count().max(1) as u32;
    crate::agent::state_machine::FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: 0,
            new_lines: count,
            content: content.to_string(),
            original: String::new(),
            updated: content.to_string(),
        }],
        status: "pending".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diffs_supports_structured_agent_changes() {
        let response = r#"```agent-changes
{
  "changes": [
    {
      "type": "edit",
      "file": "src/app.ts",
      "rationale": "rename value",
      "hunks": [
        {
          "original": "const value = 1;",
          "updated": "const value = 2;"
        }
      ]
    },
    {
      "type": "create",
      "file": "src/new.ts",
      "rationale": "add helper",
      "content": "export const helper = true;\n"
    }
  ]
}
```"#;

        let diffs = parse_diffs(response);

        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].file, "src/app.ts");
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
        assert_eq!(diffs[1].file, "src/new.ts");
        assert_eq!(diffs[1].hunks[0].updated, "export const helper = true;\n");
    }

    #[test]
    fn parse_diffs_keeps_legacy_diff_block_support() {
        let response = r#"```diff:src/app.ts
<<<<<<< ORIGINAL
const value = 1;
=======
const value = 2;
>>>>>>> UPDATED
```"#;

        let diffs = parse_diffs(response);

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file, "src/app.ts");
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
    }
}
