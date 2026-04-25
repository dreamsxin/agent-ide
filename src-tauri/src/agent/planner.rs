use crate::services::llm_client::{ChatMessage, LlmClient};
use crate::agent::state_machine::TaskStep;
use tokio::sync::mpsc;

const PLANNER_PROMPT: &str = r#"You are an expert software engineering planner. Analyze the task and produce an execution plan.

## Output Format
Output exactly one plan block with steps:

```plan
[STEP] title="Short step description" type="create|edit|run|test|analyze"
[STEP] title="Another step" type="create|edit|run|test|analyze"
```

Step types: create=make new file, edit=modify file, run=execute command, test=write tests, analyze=research/review.

## Rules
1. Steps must be concrete: "Create src/utils.ts with helper functions", NOT "Add utilities"
2. Logical order: analysis first, then implementation, then testing
3. Aim for 1-5 steps per task
4. Output ONLY the plan block

## Example
```plan
[STEP] title="Create src/api.ts with fetch wrapper" type="create"
[STEP] title="Update src/App.tsx to use new API module" type="edit"
```"#;

/// Parse steps from LLM response
pub fn parse_plan(response: &str) -> Vec<TaskStep> {
    let mut steps: Vec<TaskStep> = Vec::new();

    // Format 1: [STEP] title="..." type="..."
    for line in response.lines() {
        let line = line.trim();
        if let Some(content) = line.strip_prefix("[STEP]") {
            let mut title = String::new();
            let mut step_type = "edit".to_string();

            for part in content.split_whitespace() {
                if let Some(t) = part.strip_prefix("title=") {
                    title = t.trim_matches('"').trim_matches('\'').to_string();
                }
                if let Some(t) = part.strip_prefix("type=") {
                    step_type = t.trim_matches('"').trim_matches('\'').to_string();
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

    // Fallback: try numbered or dashed list
    if steps.is_empty() {
        for line in response.lines() {
            let line = line.trim().to_string();
            if line.is_empty() { continue; }

            let title: Option<String> = if line.starts_with("- ") || line.starts_with("* ") {
                Some(line[2..].to_string())
            } else {
                let chars: Vec<char> = line.chars().collect();
                let mut idx = 0usize;
                while idx < chars.len() && chars[idx].is_ascii_digit() { idx += 1; }
                if idx > 0 && idx < chars.len() && (chars[idx] == '.' || chars[idx] == ')') {
                    let text: String = chars[idx+1..].iter().collect();
                    let text = text.trim().to_string();
                    if !text.is_empty() { Some(text) } else { None }
                } else {
                    None
                }
            };

            if let Some(title_str) = title {
                let step_title: String;
                let step_type: String;
                if let Some(idx) = title_str.rfind('(') {
                    let t = title_str[..idx].trim().to_string();
                    let ty = title_str[idx+1..].trim_end_matches(')').trim().to_lowercase();
                    let valid = ["create","edit","run","test","analyze"].contains(&ty.as_str());
                    step_title = t;
                    step_type = if valid { ty } else { "edit".to_string() };
                } else {
                    step_title = title_str;
                    step_type = "edit".to_string();
                }

                if !step_title.is_empty() {
                    steps.push(TaskStep {
                        id: uuid::Uuid::new_v4().to_string(),
                        title: step_title,
                        step_type,
                        status: "todo".to_string(),
                        logs: Vec::new(),
                    });
                }
            }
        }
    }

    // Fallback: markdown headers
    if steps.is_empty() {
        for line in response.lines() {
            let line = line.trim();
            if (line.starts_with("##") || line.starts_with("###")) && !line.starts_with("####") {
                let title = line.trim_start_matches('#').trim().to_string();
                if title.len() > 3 {
                    steps.push(TaskStep {
                        id: uuid::Uuid::new_v4().to_string(),
                        title,
                        step_type: "edit".to_string(),
                        status: "todo".to_string(),
                        logs: Vec::new(),
                    });
                }
            }
        }
    }

    // Last resort
    if steps.is_empty() {
        steps.push(TaskStep {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Analyze requirements and implement solution".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
        });
    }

    steps
}

/// Call LLM for task planning
pub async fn plan_task(
    llm: &LlmClient,
    user_prompt: &str,
    context: &str,
    tx: mpsc::Sender<String>,
) -> Result<(Vec<TaskStep>, String), String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: PLANNER_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: if context.is_empty() {
                format!("Task:\n{}", user_prompt)
            } else {
                format!("Task:\n{}\n\nProject Context:\n{}", user_prompt, context)
            },
        },
    ];

    let full_response = llm.stream_chat(messages, tx).await?;
    let steps = parse_plan(&full_response);

    Ok((steps, full_response))
}
