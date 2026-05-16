use crate::agent::executor;
use crate::agent::state_machine::{DiffProvenance, FileDiff, TaskStep};
use crate::services::llm_client::LlmClient;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct AgentStepExecution {
    pub step: TaskStep,
    pub response: String,
    pub diffs: Vec<FileDiff>,
}

pub async fn execute_agent_steps<FStart, FFinish, FToken>(
    llm: &LlmClient,
    prompt: &str,
    context_text: &str,
    workspace_path: &Path,
    steps: &[TaskStep],
    cancel_flag: Arc<AtomicBool>,
    mut on_step_started: FStart,
    mut on_step_finished: FFinish,
    mut token_sender: FToken,
) -> Result<Vec<AgentStepExecution>, String>
where
    FStart: FnMut(usize, usize, &TaskStep),
    FFinish: FnMut(&TaskStep, &str, &[FileDiff]),
    FToken: FnMut(&TaskStep) -> mpsc::Sender<String>,
{
    let mut results = Vec::new();
    let total = steps.len();

    for (index, step) in steps.iter().enumerate() {
        on_step_started(index, total, step);
        let step_context = build_step_context(prompt, step, context_text, workspace_path);
        let response = executor::execute_step(
            llm,
            &step.title,
            &step_context,
            cancel_flag.clone(),
            token_sender(step),
        )
        .await?;
        let diffs = executor::parse_diffs(&response);
        on_step_finished(step, &response, &diffs);
        results.push(AgentStepExecution {
            step: step.clone(),
            response,
            diffs,
        });
    }

    Ok(results)
}

pub fn build_step_context(
    prompt: &str,
    step: &TaskStep,
    context_text: &str,
    workspace_path: &Path,
) -> String {
    let mut step_context = format!(
        "Task: {}\nStep: {}\nType: {}\nContext:\n{}",
        prompt, step.title, step.step_type, context_text
    );

    let paths_to_try = candidate_paths_for_step(step, workspace_path);
    let mut found_names: Vec<String> = Vec::new();
    for path in &paths_to_try {
        if !path.exists() || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if found_names.iter().any(|found| found == name) {
            continue;
        }
        if let Ok(file_content) = fs::read_to_string(path) {
            step_context.push_str(&format!(
                "\n\n--- File: {} ---\n```\n{}\n```",
                name, file_content
            ));
            found_names.push(name.to_string());
        }
    }

    if !found_names.is_empty() {
        step_context.push_str("\n\n(File contents above are current - base your diff on them)");
    }

    step_context
}

fn candidate_paths_for_step(step: &TaskStep, workspace_path: &Path) -> Vec<PathBuf> {
    let mut paths_to_try: Vec<PathBuf> = Vec::new();
    for word in step.title.split_whitespace() {
        let candidate = word.trim_matches(|ch: char| {
            !ch.is_alphanumeric() && ch != '.' && ch != '/' && ch != '\\' && ch != '-' && ch != '_'
        });
        if candidate.contains('.') && candidate.len() > 3 {
            paths_to_try.push(workspace_path.join(candidate));
            paths_to_try.push(PathBuf::from(candidate));
        }
    }

    if !paths_to_try.is_empty() {
        return paths_to_try;
    }

    let source_extensions = ["js", "ts", "jsx", "tsx", "py", "rs", "go", "java"];
    if let Ok(entries) = fs::read_dir(workspace_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                if source_extensions.contains(&ext) {
                    paths_to_try.push(path);
                }
            }
        }
    }
    paths_to_try
}

pub fn format_single_step_prompt(step: &TaskStep, extra_prompt: Option<&str>) -> String {
    let mut lines = vec![
        format!("Run this Agent plan step only: {}", step.title),
        format!("Step type: {}", step.step_type),
        format!("Scope: {}", step.scope.as_deref().unwrap_or("workspace")),
        format!(
            "Execution mode: {}",
            step.execution_mode.as_deref().unwrap_or("diff")
        ),
    ];
    if let Some(extra_prompt) = extra_prompt.filter(|value| !value.trim().is_empty()) {
        lines.push("Additional instruction:".to_string());
        lines.push(extra_prompt.to_string());
    }
    lines.push(
        "Return reviewable Agent IDE diffs when code changes are needed. Do not run unrelated plan steps."
            .to_string(),
    );
    lines.join("\n")
}

pub fn attach_step_provenance(
    diffs: &mut [FileDiff],
    step: &TaskStep,
    regenerated_from_diff_id: Option<&str>,
    regenerated_from_hunk_index: Option<usize>,
) {
    for diff in diffs {
        let provenance = diff.provenance.get_or_insert_with(|| DiffProvenance {
            protocol: "unknown".to_string(),
            operation: "unknown".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        });
        provenance.source_role = Some("agent-step".to_string());
        provenance.source_stage = Some(step.title.clone());
        provenance.regenerated_from_diff_id = regenerated_from_diff_id.map(str::to_string);
        provenance.regenerated_from_hunk_index = regenerated_from_hunk_index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state_machine::DiffHunk;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn step(title: &str) -> TaskStep {
        TaskStep {
            id: "s1".to_string(),
            title: title.to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("active_file".to_string()),
            execution_mode: Some("fix".to_string()),
        }
    }

    #[test]
    fn step_prompt_includes_scope_and_mode() {
        let prompt = format_single_step_prompt(&step("Fix parser"), Some("Use more context"));

        assert!(prompt.contains("Fix parser"));
        assert!(prompt.contains("Scope: active_file"));
        assert!(prompt.contains("Execution mode: fix"));
        assert!(prompt.contains("Use more context"));
    }

    #[test]
    fn step_context_includes_matching_workspace_file() {
        let root = std::env::temp_dir().join(format!(
            "agent-runtime-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("parser.ts"), "export const value = 1;\n").unwrap();

        let context = build_step_context(
            "Fix parser",
            &step("Update parser.ts error handling"),
            "Base context",
            &root,
        );

        assert!(context.contains("--- File: parser.ts ---"));
        assert!(context.contains("export const value = 1;"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn step_provenance_records_regeneration_source() {
        let step = step("Regenerate stale hunk");
        let mut diffs = vec![FileDiff {
            id: "d2".to_string(),
            file: "src/app.ts".to_string(),
            base_hash: None,
            provenance: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: "old".to_string(),
                updated: "new".to_string(),
                status: None,
            }],
            status: "pending".to_string(),
        }];

        attach_step_provenance(&mut diffs, &step, Some("d1"), Some(2));

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("agent-step"));
        assert_eq!(
            provenance.source_stage.as_deref(),
            Some("Regenerate stale hunk")
        );
        assert_eq!(provenance.regenerated_from_diff_id.as_deref(), Some("d1"));
        assert_eq!(provenance.regenerated_from_hunk_index, Some(2));
    }
}
