use crate::agent::multi_agent::AgentRole;
use crate::agent::state_machine::{DiffHunkProvenance, DiffProvenance, FileDiff, SddArtifact};
use crate::services::llm_client::{
    synthesize_agent_changes_block, ChatMessage, LlmClient, LlmStreamOutput, LlmToolCall,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc;

/// 将原生工具调用合并进响应文本：追加合成的 agent-changes 围栏块，
/// 使下游 parse_diffs 管线无需感知传输方式。
fn merge_tool_call_output(output: LlmStreamOutput) -> String {
    let mut content = output.content;
    if !output.tool_calls.is_empty() {
        if let Some(block) = synthesize_agent_changes_block(&output.tool_calls) {
            content.push_str(&block);
        }
    }
    content
}

/// 外部工具执行入口。当前唯一实现来源是 MCP server 发现的工具。
///
/// Agent 内置的 `emit_agent_changes` / `emit_sdd_draft` 不走这里：
/// 它们是输出协议，而不是需要回传结果的副作用调用。
#[async_trait]
pub trait ToolInvoker: Send + Sync {
    /// 该工具名是否由本 invoker 处理
    fn handles(&self, tool_name: &str) -> bool;

    /// 执行工具并返回回传给模型的文本结果
    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String>;
}

/// 单次 LLM 调用中允许的最大工具回合数。
///
/// 从 4 提到 12：以前唯一的工具来源是 MCP，回合数少无所谓；现在内置了只读的
/// 工作区工具，"搜索 → 读几个文件 → 再改" 是常规流程，4 回合会在探索途中被
/// 截断，模型只能靠猜写 ORIGINAL 段。真正的成本闸门是 per-run token 上限
/// （`RunUsageMeter`），这个常量只是防死循环的兜底。
pub const MAX_TOOL_ITERATIONS: usize = 12;

/// 从模型返回的工具调用中挑出需要真正执行的外部工具调用。
/// 内置输出协议工具（`emit_agent_changes` / `emit_sdd_draft`）不在其中。
fn select_external_calls(
    calls: &[LlmToolCall],
    invoker: Option<&dyn ToolInvoker>,
) -> Vec<LlmToolCall> {
    match invoker {
        Some(invoker) => calls
            .iter()
            .filter(|call| invoker.handles(&call.name))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// 带外部工具回合的 LLM 调用。
///
/// 每一轮：请求 → 若模型调用了外部工具则执行并把结果作为 `role: "tool"` 消息回传 → 继续请求。
/// 未启用 invoker、或模型没有调用外部工具时，行为与单次 `stream_chat_with_tools` 完全一致。
///
/// 返回值带上本轮之后新增的消息（assistant 的工具调用、`tool` 结果、最终回答）。
/// 以前这些消息是这个函数的局部变量，`return` 时一起丢掉，只剩扁平文本 ——
/// 于是工具到底返回了什么，出了这个函数就没人知道了。
async fn stream_with_tool_loop(
    llm: &LlmClient,
    mut messages: Vec<ChatMessage>,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<StageOutcome, String> {
    let prompt_len = messages.len();
    let mut merged = String::new();

    for iteration in 0..=MAX_TOOL_ITERATIONS {
        let output = llm
            .stream_chat_with_tools(messages.clone(), cancel_flag.clone(), tx.clone())
            .await?;

        let external = select_external_calls(&output.tool_calls, invoker);

        let is_last_iteration = iteration == MAX_TOOL_ITERATIONS;
        if external.is_empty() || is_last_iteration {
            let mut final_text = merge_tool_call_output(output);
            if is_last_iteration && !external.is_empty() {
                final_text.push_str(&format!(
                    "\n\n[agent-ide] Tool loop stopped after {} rounds; remaining tool calls were not executed.\n",
                    MAX_TOOL_ITERATIONS
                ));
            }
            merged.push_str(&final_text);
            let mut transcript = messages.split_off(prompt_len);
            transcript.push(ChatMessage::assistant(final_text));
            return Ok(StageOutcome {
                text: merged,
                transcript,
            });
        }

        // 保留本轮文本输出，模型可能同时给出解释和工具调用
        merged.push_str(&output.content);
        messages.push(ChatMessage::assistant_tool_calls(
            output.content.clone(),
            &output.tool_calls,
        ));

        let invoker = invoker.expect("external calls only collected when invoker is present");
        for call in &external {
            if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("Agent task cancelled".to_string());
            }
            // 工具失败也回传给模型，让它自行降级而不是直接中断整个 stage
            let result = match invoker.invoke(&call.name, &call.arguments).await {
                Ok(result) => result,
                Err(error) => format!("Tool call failed: {}", error),
            };
            messages.push(ChatMessage::tool_result(call.id.clone(), result));
        }
    }

    // 循环里每条路径都 return 了，这里只为满足类型检查
    let transcript = messages.split_off(prompt_len);
    Ok(StageOutcome {
        text: merged,
        transcript,
    })
}

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
1. If a tool for reading or searching the workspace is available, use it to read the exact
   current text of any file you intend to edit before writing a diff. Never guess an ORIGINAL
   section — a mismatch makes the change unappliable.
2. If a tool for running the project's check commands is available, use it to see the real
   failure output before deciding what to change, and cite what it reported. Do not describe a
   check as passing unless you ran it.
3. If a tool for writing workspace files is available, prefer it over emitting a diff, and then
   run the checks again to confirm the change works. Read the file first: a write replaces the
   whole file rather than patching it. If no write tool is offered, the user has chosen to review
   changes before they land — emit diffs instead.
4. Output ONLY code and diffs — no explanations unless no code change is needed
5. Each diff block must have exactly one ORIGINAL and one UPDATED section
6. For edits: show EXACT original code that needs to be replaced
7. Be precise — copy the original code exactly as it appears

Respond now with the implementation."#;

/// 执行单个步骤：调用 LLM 生成代码变更
pub async fn execute_step(
    llm: &LlmClient,
    step: &str,
    context: &str,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    let messages = vec![
        ChatMessage::system(EXECUTOR_PROMPT),
        ChatMessage::user(format!(
            "Step to execute: {}\n\nContext:\n{}\n\nProvide the implementation (code/diff only):",
            step, context
        )),
    ];

    stream_with_tool_loop(llm, messages, invoker, cancel_flag, tx)
        .await
        .map(|outcome| outcome.text)
}

/// 单条消息带进下一个 stage 时的内容上限
const MAX_CARRIED_MESSAGE_CHARS: usize = 6_000;
/// 整条线程带进下一个 stage 时的总字符上限
const MAX_CARRIED_THREAD_CHARS: usize = 24_000;

/// 一个 stage 跑完之后的产出。
///
/// `text` 是扁平文本，给人看、给 diff 解析用；`transcript` 是这个 stage 真实发生过的
/// assistant / tool 消息，按顺序排列，交给下一个 stage 当历史。
///
/// 分两个字段是因为它们丢失的东西不同：`text` 里从来没有工具返回值
/// （`merge_tool_call_output` 只取模型自己的文本），所以在此之前，下一个 stage 看不到
/// "跑了 npm test，输出是这些"，只能看到模型转述的版本 —— 而转述恰恰是不可信的那部分。
#[derive(Clone, Debug, Default)]
pub struct StageOutcome {
    pub text: String,
    pub transcript: Vec<ChatMessage>,
}

/// 把上游 stage 的消息线程裁进预算。
///
/// 之前这里是 `join_prior_outputs`：把各阶段输出拼成一段 prose 塞进 user 消息。
/// 换成真实消息之后，工具结果能原样传下去，但两条硬约束必须守住：
///
/// 1. 带 `tool_calls` 的 assistant 消息和它后面的 `tool` 结果必须同进同出。供应商
///    要求两者配对，只留一半会让整个请求 400 —— 那不是"少点历史"，是这一 stage 直接失败。
/// 2. 丢掉了内容就要说出来。缺一段而不声明，模型会把节选当成完整历史，
///    并据此断言"前面已经验证过了"。
///
/// 预算本身仍然是必要的：这条线程不经过上下文预算（预算只裁 `context`），
/// 阶段一多就会无界增长，把真正有用的项目上下文挤出窗口。
pub fn bound_transcript(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    bound_transcript_with_limits(
        messages,
        MAX_CARRIED_MESSAGE_CHARS,
        MAX_CARRIED_THREAD_CHARS,
    )
}

/// 截断一条要带进下一个 stage 的消息：保留第一行，其余按尾部截断。
///
/// `truncate_for_prompt` 保尾部，这对结论和 diff 是对的（它们都在末尾）。但出处标签
/// `[Stage / role]` 由 orchestrator 加在**头部**，于是任何超过上限的阶段输出，第一个被
/// 丢掉的就是归属信息 —— 恰好是长输出最需要标签的时候。所以第一行单独保住。
fn truncate_carried_message(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    match value.split_once('\n') {
        Some((first, rest)) => {
            let first_len = first.chars().count();
            // 第一行本身就超预算时没什么可保的，退回统一的尾部截断
            if first_len + 1 >= max_chars {
                return crate::services::verification::truncate_for_prompt(value, max_chars);
            }
            let budget = max_chars - first_len - 1;
            format!(
                "{}\n{}",
                first,
                crate::services::verification::truncate_for_prompt(rest, budget)
            )
        }
        None => crate::services::verification::truncate_for_prompt(value, max_chars),
    }
}

fn bound_transcript_with_limits(
    messages: &[ChatMessage],
    max_message_chars: usize,
    max_thread_chars: usize,
) -> Vec<ChatMessage> {
    // 分组：`tool` 消息永远归到前一条 assistant 上，这样丢弃只会按组发生
    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    for message in messages {
        let mut trimmed = message.clone();
        trimmed.content = truncate_carried_message(&message.content, max_message_chars);
        match groups.last_mut() {
            Some(group) if message.role == "tool" => group.push(trimmed),
            // 开头就是 `tool` 说明它的 assistant 调用不在这段历史里。带上去会让供应商
            // 拒掉整个请求（tool 必须紧跟发起它的调用），所以直接丢掉 —— 少一条历史，
            // 而不是这一 stage 整体失败。当前没有入口会产生这种线程，这里是护栏。
            None if message.role == "tool" => continue,
            _ => groups.push(vec![trimmed]),
        }
    }

    let mut kept: Vec<Vec<ChatMessage>> = Vec::new();
    let mut used = 0usize;
    for group in groups.iter().rev() {
        let cost: usize = group
            .iter()
            .map(|message| message.content.chars().count())
            .sum();
        // 最近的一组即使超预算也保留：紧邻的上一步是下一个 stage 最依赖的东西，
        // 一条历史都不给比给一条超长历史更糟
        if !kept.is_empty() && used + cost > max_thread_chars {
            break;
        }
        used += cost;
        kept.push(group.clone());
    }
    kept.reverse();

    let omitted = groups.len() - kept.len();
    let mut result: Vec<ChatMessage> = Vec::new();
    if omitted > 0 {
        result.push(ChatMessage::system(format!(
            "[agent-ide] {} earlier exchange(s) from previous stages were omitted to fit the context budget. Do not assume work you cannot see here was done.",
            omitted
        )));
    }
    result.extend(kept.into_iter().flatten());
    result
}

pub async fn execute_stage(
    llm: &LlmClient,
    role: AgentRole,
    stage_name: &str,
    user_prompt: &str,
    context: &str,
    prior_transcript: &[ChatMessage],
    pending_diffs: &str,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<StageOutcome, String> {
    let output_rules = match role {
        AgentRole::Architect => "Output a concise implementation plan. Do not output code diffs.",
        AgentRole::Designer => {
            r#"Output one SDD Markdown draft and no source-code diffs. Wrap the document in an `sdd` fence:

```sdd
---
type: sdd
title: Clear design title
version: 1
date: YYYY-MM-DD
status: draft
module: module-or-feature-name
---

# Clear design title

## Problem
...

## Goals
...

## Non-Goals
...

## Proposed Design
...

## User Flows
...

## Interfaces and Data
...

## Acceptance Criteria
...

## Risks
...

## Implementation Notes
...
```

The draft must be specific enough for a later code-mode Agent run to implement it."#
        }
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

    // 结构：角色系统提示 → 本 stage 的任务消息 → 上游 stage 的真实消息 → 开跑指令。
    // 上游消息里只有 assistant / tool 角色，所以"第一条 user"仍然是本 stage 的任务，
    // 依赖这一点的 mock provider 和本地模型扁平化路径都不受影响。
    let mut messages = vec![
        ChatMessage::system(format!("{}\n\n{}", role.system_prompt(), output_rules)),
        ChatMessage::user(format!(
            "Pipeline stage: {}\nRole: {}\n\nUser task:\n{}\n\nProject context:\n{}\n\nActual pending diffs for review:\n{}",
            stage_name,
            role.to_string(),
            user_prompt,
            context,
            if pending_diffs.trim().is_empty() {
                "No pending diffs."
            } else {
                pending_diffs
            },
        )),
    ];
    messages.extend(bound_transcript(prior_transcript));
    messages.push(ChatMessage::user(if prior_transcript.is_empty() {
        "This is the first stage of the run; there is no prior stage work. Run this stage now."
    } else {
        "Prior stage work is in the messages above, including the actual tool results rather than a retelling of them. Run this stage now."
    }));

    stream_with_tool_loop(llm, messages, invoker, cancel_flag, tx).await
}

/// 从 LLM 响应中解析 diff 块
pub fn parse_diffs(response: &str) -> Vec<FileDiff> {
    parse_diffs_with_diagnostics(response).diffs
}

pub fn parse_sdd_artifact(
    response: &str,
    prompt: &str,
    source_run_id: Option<String>,
) -> SddArtifact {
    let raw_markdown = extract_sdd_markdown(response);
    let (mut frontmatter, body) = split_frontmatter(&raw_markdown);
    let title = frontmatter
        .get("title")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| extract_markdown_title(&body))
        .unwrap_or_else(|| summarize_prompt_as_title(prompt));
    let slug = frontmatter
        .get("slug")
        .cloned()
        .filter(|value| is_safe_slug(value))
        .unwrap_or_else(|| slugify(&title));

    frontmatter
        .entry("type".to_string())
        .or_insert_with(|| "sdd".to_string());
    frontmatter
        .entry("title".to_string())
        .or_insert_with(|| title.clone());
    frontmatter
        .entry("version".to_string())
        .or_insert_with(|| "1".to_string());
    frontmatter
        .entry("date".to_string())
        .or_insert_with(|| chrono::Utc::now().date_naive().to_string());
    frontmatter
        .entry("status".to_string())
        .or_insert_with(|| "draft".to_string());
    frontmatter
        .entry("module".to_string())
        .or_insert_with(|| slug.clone());

    let markdown = format!(
        "---\n{}---\n\n{}",
        format_frontmatter(&frontmatter),
        body.trim_start()
    );
    SddArtifact {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        slug,
        frontmatter,
        markdown,
        source_run_id,
        review_findings: Vec::new(),
        status: "draft".to_string(),
    }
}

pub fn extract_review_findings(response: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let mut in_findings = false;
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_findings = trimmed
                .trim_start_matches('#')
                .trim()
                .to_ascii_lowercase()
                .contains("finding");
            continue;
        }
        if in_findings && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            let finding = trimmed[2..].trim();
            if !finding.is_empty() {
                findings.push(finding.to_string());
            }
        }
    }
    findings
}

fn extract_sdd_markdown(response: &str) -> String {
    let lines: Vec<&str> = response.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let rest = trimmed.strip_prefix("```");
        if matches!(rest, Some("sdd") | Some("markdown:sdd") | Some("md:sdd")) {
            let mut block = Vec::new();
            index += 1;
            while index < lines.len() && lines[index].trim() != "```" {
                block.push(lines[index]);
                index += 1;
            }
            return block.join("\n");
        }
        index += 1;
    }
    response.trim().to_string()
}

fn split_frontmatter(markdown: &str) -> (BTreeMap<String, String>, String) {
    let normalized = markdown.trim_start();
    if !normalized.starts_with("---\n") && !normalized.starts_with("---\r\n") {
        return (BTreeMap::new(), normalized.to_string());
    }

    let mut lines = normalized.lines();
    let _ = lines.next();
    let mut frontmatter = BTreeMap::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }
        if in_frontmatter {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    frontmatter.insert(key.to_string(), value.to_string());
                }
            }
        } else {
            body_lines.push(line);
        }
    }
    (frontmatter, body_lines.join("\n"))
}

fn format_frontmatter(frontmatter: &BTreeMap<String, String>) -> String {
    frontmatter
        .iter()
        .map(|(key, value)| format!("{}: {}\n", key, value))
        .collect()
}

fn extract_markdown_title(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let title = line.trim().strip_prefix("# ")?;
        let title = title.trim();
        (!title.is_empty()).then(|| title.to_string())
    })
}

fn summarize_prompt_as_title(prompt: &str) -> String {
    let title = prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Design Specification")
        .chars()
        .take(72)
        .collect::<String>();
    if title.trim().is_empty() {
        "Design Specification".to_string()
    } else {
        title.trim().to_string()
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        format!(
            "design-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    } else {
        slug
    }
}

pub fn is_safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && !value.contains("..")
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

    /// 带进下一个 stage 的线程是绕过上下文预算直接进请求的，所以它自己必须有上限：
    /// 最近的交换原样保留，更早的整组丢弃并写明丢了多少。
    #[test]
    fn carried_thread_keeps_recent_exchanges_and_states_what_it_dropped() {
        let old = "O".repeat(50);
        let messages = vec![
            ChatMessage::assistant(old.clone()),
            ChatMessage::assistant("second".to_string()),
            ChatMessage::assistant("third".to_string()),
        ];

        let bounded = bound_transcript_with_limits(&messages, 6_000, 12);

        // 最近的两条一个字都不能少：下一个 stage 最依赖紧邻的上一步
        let contents: Vec<&str> = bounded
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert!(contents.contains(&"second"), "{:?}", contents);
        assert!(contents.contains(&"third"), "{:?}", contents);
        assert!(!contents.contains(&old.as_str()), "{:?}", contents);

        // 丢弃量必须写出来：静默丢弃会让模型以为自己看到了完整历史，
        // 然后断言"前面已经验证过了"
        assert_eq!(bounded[0].role, "system");
        assert!(
            bounded[0].content.contains("1 earlier exchange(s)"),
            "{}",
            bounded[0].content
        );
    }

    /// 单条消息也要有上限，否则一个几十万字符的输出能独自撑爆请求。
    #[test]
    fn carried_thread_truncates_an_oversized_single_message() {
        let huge = "T".repeat(500);
        // 用 assistant 而不是 tool：开头的 tool 消息会被上面那条护栏丢掉，
        // 而这条测试要验证的是"单条超长消息被截断"，角色是无关变量
        let bounded = bound_transcript_with_limits(&[ChatMessage::assistant(huge.clone())], 20, 0);

        assert_eq!(bounded.len(), 1);
        assert!(
            bounded[0].content.contains("earlier character(s) omitted"),
            "{}",
            bounded[0].content
        );
        assert!(bounded[0].content.chars().count() < huge.chars().count());
    }

    /// 供应商要求带 `tool_calls` 的 assistant 消息后面必须紧跟对应的 `tool` 结果。
    /// 只丢一半不是"少点历史"，而是整个请求 400、这一 stage 直接失败 ——
    /// 所以裁剪只能按整组进行。
    #[test]
    fn carried_thread_never_splits_a_tool_call_from_its_result() {
        let call = LlmToolCall {
            id: "call-1".to_string(),
            name: "workspace_read_file".to_string(),
            arguments: "{}".to_string(),
        };
        let messages = vec![
            ChatMessage::assistant("earlier chatter".to_string()),
            ChatMessage::assistant_tool_calls("reading the file".to_string(), &[call]),
            ChatMessage::tool_result("call-1", "file contents".to_string()),
        ];

        // 预算只够最后一组
        let bounded = bound_transcript_with_limits(&messages, 6_000, 30);

        let roles: Vec<&str> = bounded
            .iter()
            .map(|message| message.role.as_str())
            .collect();
        assert_eq!(roles, vec!["system", "assistant", "tool"], "{:?}", roles);
        assert_eq!(bounded[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn carried_thread_is_empty_when_no_stage_has_run() {
        assert!(bound_transcript(&[]).is_empty());
    }

    /// orchestrator 把 `[Stage / role]` 标签加在消息**头部**，而尾部截断会先吃掉头部。
    /// 于是超长阶段输出恰好丢掉归属 —— 长输出正是最需要知道"这是谁说的"的时候。
    #[test]
    fn carried_thread_keeps_the_provenance_label_of_a_long_message() {
        let long = format!("[Architect / architect]\n{}", "detail\n".repeat(400));
        let long_len = long.chars().count();

        let bounded = bound_transcript_with_limits(
            &[ChatMessage::assistant(long)],
            120,
            crate::agent::executor::MAX_CARRIED_THREAD_CHARS,
        );

        assert_eq!(bounded.len(), 1);
        assert!(
            bounded[0].content.starts_with("[Architect / architect]\n"),
            "{}",
            bounded[0].content
        );
        // 仍然要保尾部并写明省略量：结论和 diff 都在末尾
        assert!(
            bounded[0].content.contains("earlier character(s) omitted"),
            "{}",
            bounded[0].content
        );
        assert!(
            bounded[0].content.ends_with("detail\n"),
            "{}",
            bounded[0].content
        );
        // 不断言精确长度：`truncate_for_prompt` 会在预算之外再加一行省略标记，
        // 写死一个数字只会变成测实现细节。够短就行。
        assert!(
            bounded[0].content.chars().count() < long_len / 4,
            "{}",
            bounded[0].content
        );
    }

    /// 供应商要求每条 `tool` 消息前面必须有发起它的 assistant 调用。分组逻辑依赖
    /// "线程不会以 tool 消息开头"这个前提；这条测试把前提本身钉住，避免以后某个
    /// 新的续跑入口传进一段以 tool 开头的历史，导致整个请求被拒。
    #[test]
    fn carried_thread_never_emits_a_tool_message_without_its_assistant_call() {
        let orphan = vec![
            ChatMessage::tool_result("call-1", "orphaned result".to_string()),
            ChatMessage::assistant("later answer".to_string()),
        ];

        let bounded = bound_transcript(&orphan);

        let first_tool = bounded.iter().position(|message| message.role == "tool");
        if let Some(index) = first_tool {
            assert!(index > 0, "tool 消息不能是第一条: {:?}", bounded);
            assert!(
                bounded[index - 1].tool_calls.is_some(),
                "tool 消息前面必须是发起它的 assistant 调用: {:?}",
                bounded
            );
        }
    }

    struct RecordingInvoker {
        prefix: &'static str,
        calls: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl RecordingInvoker {
        fn new(prefix: &'static str) -> Self {
            Self {
                prefix,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolInvoker for RecordingInvoker {
        fn handles(&self, tool_name: &str) -> bool {
            tool_name.starts_with(self.prefix)
        }

        async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push((tool_name.to_string(), arguments.to_string()));
            Ok("ok".to_string())
        }
    }

    fn call(name: &str) -> LlmToolCall {
        LlmToolCall {
            id: format!("call_{}", name),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    #[test]
    fn external_calls_exclude_builtin_output_protocol_tools() {
        let invoker = RecordingInvoker::new("mcp__");
        let calls = vec![
            call("emit_agent_changes"),
            call("mcp__files__read"),
            call("emit_sdd_draft"),
            call("mcp__git__log"),
        ];

        let selected = select_external_calls(&calls, Some(&invoker));

        assert_eq!(
            selected
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
            vec!["mcp__files__read", "mcp__git__log"]
        );
    }

    #[test]
    fn external_calls_are_empty_without_invoker() {
        assert!(select_external_calls(&[call("mcp__files__read")], None).is_empty());
    }

    #[test]
    fn merge_tool_call_output_appends_agent_changes_block() {
        let merged = merge_tool_call_output(LlmStreamOutput {
            content: "Applying the rename.".to_string(),
            tool_calls: vec![crate::services::llm_client::LlmToolCall {
                id: "call_1".to_string(),
                name: "emit_agent_changes".to_string(),
                arguments: r#"{"version":1,"changes":[{"type":"edit","file":"src/app.ts","hunks":[{"original":"const a = 1;","updated":"const a = 2;"}]}]}"#.to_string(),
            }],
            usage: None,
        });

        assert!(merged.starts_with("Applying the rename."));
        assert!(merged.contains("```agent-changes"));

        // 合成块必须能被现有 parse_diffs 管线解析（传输方式对下游透明）
        let diffs = parse_diffs(&merged);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file, "src/app.ts");
    }

    #[test]
    fn merge_tool_call_output_keeps_plain_content_when_no_usable_calls() {
        let base = LlmStreamOutput {
            content: "Just prose.".to_string(),
            tool_calls: vec![crate::services::llm_client::LlmToolCall {
                id: "call_2".to_string(),
                name: "emit_agent_changes".to_string(),
                arguments: r#"{"version":1,"changes":[]}"#.to_string(),
            }],
            usage: None,
        };
        assert_eq!(merge_tool_call_output(base), "Just prose.");

        let plain = LlmStreamOutput {
            content: "Just prose.".to_string(),
            tool_calls: Vec::new(),
            usage: None,
        };
        assert_eq!(merge_tool_call_output(plain), "Just prose.");
    }

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

    #[test]
    fn parse_sdd_artifact_normalizes_frontmatter_and_slug() {
        let artifact = parse_sdd_artifact(
            r#"```sdd
---
type: sdd
title: Token Budget Meter
version: 1
status: draft
module: chat
---

# Token Budget Meter

## Goals
- Show budget usage.
```"#,
            "Build token budget UI",
            Some("run-1".to_string()),
        );

        assert_eq!(artifact.title, "Token Budget Meter");
        assert_eq!(artifact.slug, "token-budget-meter");
        assert_eq!(artifact.source_run_id.as_deref(), Some("run-1"));
        assert_eq!(
            artifact.frontmatter.get("type").map(String::as_str),
            Some("sdd")
        );
        assert!(artifact.markdown.starts_with("---\n"));
    }

    #[test]
    fn parse_sdd_artifact_adds_required_frontmatter_when_missing() {
        let artifact = parse_sdd_artifact(
            "# Python LSP\n\n## Goals\n- Diagnostics",
            "Python LSP",
            None,
        );

        assert_eq!(artifact.title, "Python LSP");
        assert_eq!(artifact.slug, "python-lsp");
        assert_eq!(
            artifact.frontmatter.get("status").map(String::as_str),
            Some("draft")
        );
        assert!(artifact.markdown.contains("type: sdd"));
    }

    #[test]
    fn sdd_slug_validation_rejects_path_traversal() {
        assert!(is_safe_slug("token-budget-meter"));
        assert!(!is_safe_slug("../secret"));
        assert!(!is_safe_slug("feature/name"));
    }
}
