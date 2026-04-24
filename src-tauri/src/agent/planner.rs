use crate::services::llm_client::{ChatMessage, LlmClient};
use crate::agent::state_machine::TaskStep;
use tokio::sync::mpsc;

/// 系统提示词模板
const SYSTEM_PROMPT: &str = r#"You are an AI coding agent inside Agent IDE.
You help users design and implement code changes.

When responding to a user request, you MUST:
1. Analyze the request and the provided context
2. Output a PLAN first, listing steps in this format:
```plan
[STEP] title="step name" type="create|edit|run|test"
[STEP] title="another step" type="create|edit|run|test"
```
3. Then for each step, provide the actual code or instructions.
4. When suggesting code changes, use diff format:
```diff: path/to/file
<<<<<<< ORIGINAL
old code
=======
new code
>>>>>>> UPDATED
```

Always be concise and direct. Focus on producing actionable code."#;

/// 解析 LLM 响应，提取任务计划
pub fn parse_plan(response: &str) -> Vec<TaskStep> {
    let mut steps = Vec::new();
    for line in response.lines() {
        let line = line.trim();
        if let Some(content) = line.strip_prefix("[STEP]") {
            let mut title = String::new();
            let mut step_type = "edit".to_string();

            // 解析 title="..." 和 type="..."
            for part in content.split_whitespace() {
                if let Some(t) = part.strip_prefix("title=") {
                    title = t
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                }
                if let Some(t) = part.strip_prefix("type=") {
                    step_type = t
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                }
            }

            if !title.is_empty() {
                steps.push(TaskStep {
                    id: uuid::Uuid::new_v4().to_string(),
                    title,
                    step_type,
                    status: "todo".to_string(),
                    logs: Vec::new(),
                });
            }
        }
    }

    // 如果没有解析到步骤，创建一个通用步骤
    if steps.is_empty() {
        steps.push(TaskStep {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Analyze and implement".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
        });
    }

    steps
}

/// 调用 LLM 规划任务
pub async fn plan_task(
    llm: &LlmClient,
    user_prompt: &str,
    context: &str,
    tx: mpsc::Sender<String>,
) -> Result<(Vec<TaskStep>, String), String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!("{}\n\nContext:\n{}", user_prompt, context),
        },
    ];

    let full_response = llm.stream_chat(messages, tx).await?;
    let steps = parse_plan(&full_response);

    Ok((steps, full_response))
}
