use crate::agent::multi_agent::AgentRole;
use crate::agent::state_machine::{DiffHunkProvenance, DiffProvenance, FileDiff};
use crate::services::llm_client::{ChatMessage, LlmClient};
use serde::Deserialize;
use std::sync::{atomic::AtomicBool, Arc};
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
        AgentRole::Architect => "Output a concise implementation plan. Do not output code diffs.",
        AgentRole::Coder | AgentRole::Tester => {
            r#"When code changes are needed, prefer the Agent IDE `agent-changes` schema version 1:

```agent-changes
{
  "version": 1,
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
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "path/to/file",
      "hunkIndex": 0,
      "message": "optional reviewer finding tied to a hunk"
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
pub fn parse_diffs(response: &str) -> Vec<FileDiff> {
    parse_diffs_with_diagnostics(response).diffs
}

#[derive(Debug, Clone)]
pub struct ParsedDiffs {
    pub diffs: Vec<FileDiff>,
    pub diagnostics: Vec<String>,
}

pub fn parse_diffs_with_diagnostics(response: &str) -> ParsedDiffs {
    let mut diffs = Vec::new();
    let mut diagnostics = Vec::new();
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
                    let parsed = parse_agent_changes(&content);
                    diffs.extend(parsed.diffs);
                    diagnostics.extend(parsed.diagnostics);
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

    ParsedDiffs { diffs, diagnostics }
}

/// 检测代码块类型和文件名: 返回 (类型, 文件名)
fn detect_block_start(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("```")?;
    if rest.is_empty() {
        return None;
    }

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
    #[serde(default)]
    version: Option<u32>,
    changes: Vec<AgentChange>,
    #[serde(default)]
    findings: Vec<AgentFinding>,
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

#[derive(Debug, Deserialize)]
struct AgentFinding {
    severity: String,
    file: String,
    #[serde(rename = "hunkIndex")]
    hunk_index: Option<usize>,
    message: String,
}

fn parse_agent_changes(json: &str) -> ParsedDiffs {
    let block = match serde_json::from_str::<AgentChangesBlock>(json) {
        Ok(block) => block,
        Err(err) => {
            return ParsedDiffs {
                diffs: Vec::new(),
                diagnostics: vec![format!("agent-changes JSON parse error: {}", err)],
            };
        }
    };

    let mut diffs = Vec::new();
    let mut diagnostics = Vec::new();
    if block.version != Some(1) {
        diagnostics.push(format!(
            "agent-changes version must be 1; got {:?}",
            block.version
        ));
        return ParsedDiffs { diffs, diagnostics };
    }
    if block.changes.is_empty() {
        diagnostics.push("agent-changes must include at least one change".to_string());
        return ParsedDiffs { diffs, diagnostics };
    }
    let findings = block.findings;
    for (change_index, change) in block.changes.into_iter().enumerate() {
        let change_type = change.change_type.trim();
        let file = change.file.trim();
        if !is_valid_relative_file_path(file) {
            diagnostics.push(format!(
                "agent-changes change {} has invalid relative file path: {}",
                change_index, change.file
            ));
            continue;
        }
        let provenance = DiffProvenance {
            protocol: "agent-changes".to_string(),
            operation: normalized_operation(change_type).to_string(),
            rationale: change
                .rationale
                .clone()
                .filter(|value| !value.trim().is_empty()),
            schema_version: block.version,
            change_index: Some(change_index),
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        };

        match change_type {
            "create" | "new" => {
                if let Some(content) = change.content {
                    if !content.trim().is_empty() && change.hunks.is_none() {
                        let mut diff = make_new_file_diff(file, &content);
                        diff.provenance = Some(provenance);
                        if let Some(rationale) = change.rationale {
                            if let Some(hunk) = diff.hunks.first_mut() {
                                hunk.content =
                                    format!("rationale: {}\n\n{}", rationale, hunk.content);
                            }
                        }
                        diffs.push(diff);
                    } else {
                        diagnostics.push(format!(
                            "agent-changes create change {} must provide non-empty content and no hunks",
                            change_index
                        ));
                    }
                } else {
                    diagnostics.push(format!(
                        "agent-changes create change {} is missing content",
                        change_index
                    ));
                }
            }
            "edit" | "modify" => {
                if change.content.is_some() {
                    diagnostics.push(format!(
                        "agent-changes edit change {} must use hunks and not content",
                        change_index
                    ));
                    continue;
                };
                let Some(hunks) = change.hunks else {
                    diagnostics.push(format!(
                        "agent-changes edit change {} is missing hunks",
                        change_index
                    ));
                    continue;
                };
                let parsed_hunks: Vec<_> = hunks
                    .into_iter()
                    .enumerate()
                    .filter_map(|(hunk_index, hunk)| {
                        if hunk.original.trim().is_empty() {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} has empty original",
                                change_index, hunk_index
                            ));
                            return None;
                        }
                        if hunk.original == hunk.updated {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} does not change content",
                                change_index, hunk_index
                            ));
                            return None;
                        }
                        if hunk.updated.contains("\u{0000}") || hunk.original.contains("\u{0000}") {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} contains NUL bytes",
                                change_index, hunk_index
                            ));
                            return None;
                        }
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
                        Some(crate::agent::state_machine::DiffHunk {
                            old_start: 0,
                            old_lines: old_count,
                            new_start: 0,
                            new_lines: new_count,
                            content: change.rationale.clone().unwrap_or_default(),
                            original: hunk.original,
                            updated: hunk.updated,
                            provenance: Some(DiffHunkProvenance {
                                change_index: Some(change_index),
                                hunk_index: Some(hunk_index),
                                source_role: None,
                                source_stage: None,
                                prompt_context: Some(format!(
                                    "agent-changes change {} hunk {}",
                                    change_index, hunk_index
                                )),
                                rationale: change.rationale.clone(),
                            }),
                            status: None,
                        })
                    })
                    .collect();

                if !parsed_hunks.is_empty() {
                    diffs.push(FileDiff {
                        id: uuid::Uuid::new_v4().to_string(),
                        file: file.to_string(),
                        base_hash: change.base_hash,
                        provenance: Some(provenance),
                        hunks: parsed_hunks,
                        status: "pending".to_string(),
                    });
                } else {
                    diagnostics.push(format!(
                        "agent-changes edit change {} has no valid hunks",
                        change_index
                    ));
                }
            }
            _ => diagnostics.push(format!(
                "agent-changes change {} has unsupported type: {}",
                change_index, change.change_type
            )),
        }
    }

    attach_findings_to_hunks(&mut diffs, &findings, &mut diagnostics);

    ParsedDiffs { diffs, diagnostics }
}

fn attach_findings_to_hunks(
    diffs: &mut [FileDiff],
    findings: &[AgentFinding],
    diagnostics: &mut Vec<String>,
) {
    for (finding_index, finding) in findings.iter().enumerate() {
        if finding.message.trim().is_empty() {
            diagnostics.push(format!(
                "agent-changes finding {} has empty message",
                finding_index
            ));
            continue;
        }
        let file = finding.file.trim();
        let Some(diff) = diffs.iter_mut().find(|diff| diff.file == file) else {
            diagnostics.push(format!(
                "agent-changes finding {} references unknown file: {}",
                finding_index, finding.file
            ));
            continue;
        };
        let hunk_index = finding.hunk_index.unwrap_or(0);
        let Some(hunk) = diff.hunks.get_mut(hunk_index) else {
            diagnostics.push(format!(
                "agent-changes finding {} references missing hunk {} in {}",
                finding_index, hunk_index, finding.file
            ));
            continue;
        };
        let provenance = hunk.provenance.get_or_insert_with(|| DiffHunkProvenance {
            change_index: diff
                .provenance
                .as_ref()
                .and_then(|value| value.change_index),
            hunk_index: Some(hunk_index),
            source_role: None,
            source_stage: None,
            prompt_context: None,
            rationale: diff
                .provenance
                .as_ref()
                .and_then(|value| value.rationale.clone()),
        });
        let note = format!(
            "reviewer finding [{}]: {}",
            finding.severity.trim(),
            finding.message.trim()
        );
        provenance.prompt_context = Some(match provenance.prompt_context.as_deref() {
            Some(existing) if !existing.trim().is_empty() => format!("{}\n{}", existing, note),
            _ => note,
        });
    }
}

fn normalized_operation(change_type: &str) -> &'static str {
    match change_type {
        "new" => "create",
        "modify" => "edit",
        "create" => "create",
        "edit" => "edit",
        _ => "unknown",
    }
}

fn is_valid_relative_file_path(file: &str) -> bool {
    if file.is_empty()
        || file.contains('\0')
        || file.starts_with('/')
        || file.starts_with('\\')
        || file.contains("://")
        || std::path::Path::new(file).is_absolute()
    {
        return false;
    }

    let normalized = file.replace('\\', "/");
    !normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
}

/// 分割 diff 内容为 ORIGINAL 和 UPDATED 两部分
fn split_diff_content(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut original = Vec::new();
    let mut updated = Vec::new();
    let mut in_original = false;
    let mut in_updated = false;

    for line in lines {
        let t = line.trim();
        if t.starts_with("<<<<<<<") {
            in_original = true;
            in_updated = false;
            continue;
        }
        if t.starts_with("=======") {
            in_original = false;
            in_updated = true;
            continue;
        }
        if t.starts_with(">>>>>>>") {
            in_original = false;
            in_updated = false;
            continue;
        }
        if in_original {
            original.push(line.clone());
        } else if in_updated {
            updated.push(line.clone());
        }
    }

    (original, updated)
}

fn make_diff(file: &str, content: &str, original: &[String], updated: &[String]) -> FileDiff {
    let old_count = original
        .iter()
        .filter(|l| !l.trim().is_empty())
        .count()
        .max(1) as u32;
    let new_count = updated
        .iter()
        .filter(|l| !l.trim().is_empty())
        .count()
        .max(1) as u32;

    FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        provenance: Some(DiffProvenance {
            protocol: "legacy-diff-block".to_string(),
            operation: "edit".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        }),
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: old_count,
            new_start: 0,
            new_lines: new_count,
            content: content.to_string(),
            original: original.join("\n"),
            updated: updated.join("\n"),
            provenance: Some(DiffHunkProvenance {
                change_index: None,
                hunk_index: Some(0),
                source_role: None,
                source_stage: None,
                prompt_context: Some("legacy diff block".to_string()),
                rationale: None,
            }),
            status: None,
        }],
        status: "pending".to_string(),
    }
}

fn make_new_file_diff(file: &str, content: &str) -> FileDiff {
    let count = content.lines().count().max(1) as u32;
    FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        provenance: Some(DiffProvenance {
            protocol: "legacy-new-block".to_string(),
            operation: "create".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        }),
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: 0,
            new_lines: count,
            content: content.to_string(),
            original: String::new(),
            updated: content.to_string(),
            provenance: Some(DiffHunkProvenance {
                change_index: None,
                hunk_index: Some(0),
                source_role: None,
                source_stage: None,
                prompt_context: Some("legacy new-file block".to_string()),
                rationale: None,
            }),
            status: None,
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
  "version": 1,
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
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().protocol,
            "agent-changes"
        );
        assert_eq!(diffs[0].provenance.as_ref().unwrap().operation, "edit");
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().schema_version,
            Some(1)
        );
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().rationale.as_deref(),
            Some("rename value")
        );
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
        assert_eq!(
            diffs[0].hunks[0].provenance.as_ref().unwrap().change_index,
            Some(0)
        );
        assert_eq!(diffs[1].file, "src/new.ts");
        assert_eq!(diffs[1].provenance.as_ref().unwrap().operation, "create");
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
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().protocol,
            "legacy-diff-block"
        );
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
    }

    #[test]
    fn parse_diffs_rejects_invalid_structured_agent_changes() {
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "../outside.ts",
      "hunks": [
        { "original": "const value = 1;", "updated": "const value = 2;" }
      ]
    },
    {
      "type": "edit",
      "file": "src/same.ts",
      "hunks": [
        { "original": "const value = 1;", "updated": "const value = 1;" }
      ]
    },
    {
      "type": "create",
      "file": "src/mixed.ts",
      "content": "export {};",
      "hunks": [
        { "original": "old", "updated": "new" }
      ]
    }
  ]
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn parse_diffs_reports_structured_validation_errors() {
        let response = r#"```agent-changes
{
  "version": 2,
  "changes": []
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(parsed
            .diagnostics
            .iter()
            .any(|item| item.contains("version must be 1")));
    }

    #[test]
    fn parse_diffs_attaches_review_findings_to_hunk_provenance() {
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "src/app.ts",
      "rationale": "fix value",
      "hunks": [
        {
          "original": "const value = 1;",
          "updated": "const value = 2;"
        }
      ]
    }
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "src/app.ts",
      "hunkIndex": 0,
      "message": "verify value usage"
    }
  ]
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert_eq!(parsed.diffs.len(), 1);
        let hunk_provenance = parsed.diffs[0].hunks[0]
            .provenance
            .as_ref()
            .expect("hunk provenance");
        assert!(hunk_provenance
            .prompt_context
            .as_deref()
            .unwrap_or_default()
            .contains("verify value usage"));
    }
}
