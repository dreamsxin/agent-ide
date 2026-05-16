use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompressionMode {
    Full,
    Focused,
    Compact,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextBudget {
    #[serde(rename = "maxContextTokens")]
    pub max_context_tokens: Option<usize>,
    #[serde(rename = "reservedOutputTokens")]
    pub reserved_output_tokens: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ContextBuildOptions {
    pub compression: ContextCompressionMode,
    pub budget: Option<ContextBudget>,
}

impl ContextBuildOptions {
    pub fn new(compression: ContextCompressionMode, budget: Option<ContextBudget>) -> Self {
        Self {
            compression,
            budget,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextSourceOptions {
    #[serde(default, rename = "includeProjectTree")]
    pub include_project_tree: bool,
    #[serde(default, rename = "includeGitDiff")]
    pub include_git_diff: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextEstimateSection {
    pub id: String,
    pub label: String,
    pub chars: usize,
    #[serde(rename = "estimatedTokens")]
    pub estimated_tokens: usize,
    pub included: bool,
    pub trimmed: bool,
    #[serde(rename = "excludedReason")]
    pub excluded_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextEstimateResponse {
    pub sections: Vec<ContextEstimateSection>,
    #[serde(rename = "rawChars")]
    pub raw_chars: usize,
    #[serde(rename = "finalChars")]
    pub final_chars: usize,
    #[serde(rename = "estimatedTokens")]
    pub estimated_tokens: usize,
    #[serde(rename = "inputBudgetTokens")]
    pub input_budget_tokens: Option<usize>,
    pub trimmed: bool,
}

#[derive(Clone, Debug)]
pub struct ContextSection {
    pub id: &'static str,
    pub label: &'static str,
    pub content: String,
}

impl Default for ContextCompressionMode {
    fn default() -> Self {
        Self::Focused
    }
}

impl std::fmt::Display for ContextCompressionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Focused => write!(f, "focused"),
            Self::Compact => write!(f, "compact"),
        }
    }
}

impl ContextCompressionMode {
    pub fn from_str(mode: &str) -> Result<Self, String> {
        match mode {
            "full" => Ok(Self::Full),
            "focused" => Ok(Self::Focused),
            "compact" => Ok(Self::Compact),
            _ => Err(format!("Invalid context compression mode: {}", mode)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentContext {
    pub active_file: Option<String>,
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    pub open_files: Vec<String>,
    pub project_path: String,
    #[serde(default)]
    pub git_diff: Option<String>,
    #[serde(default)]
    pub project_tree: Option<String>,
}

impl AgentContext {
    pub fn new(project_path: &str) -> Self {
        Self {
            active_file: None,
            active_file_content: None,
            selection: None,
            open_files: Vec::new(),
            project_path: project_path.to_string(),
            git_diff: None,
            project_tree: None,
        }
    }

    pub fn enrich_from_workspace(&mut self) {
        self.enrich_from_workspace_with_sources(&ContextSourceOptions {
            include_project_tree: true,
            include_git_diff: true,
        });
    }

    pub fn enrich_from_workspace_with_sources(&mut self, sources: &ContextSourceOptions) {
        if sources.include_project_tree && self.project_tree.is_none() {
            self.project_tree = build_project_tree_summary(160, 4).ok();
        }
        if sources.include_git_diff && self.git_diff.is_none() {
            self.git_diff = build_git_diff_summary(24_000).ok();
        }
    }

    pub fn to_prompt_context(&self) -> String {
        self.to_prompt_context_with_mode(&ContextCompressionMode::Full)
    }

    pub fn to_prompt_context_with_mode(&self, mode: &ContextCompressionMode) -> String {
        self.to_prompt_context_with_options(&ContextBuildOptions::new(mode.clone(), None))
    }

    pub fn to_prompt_context_with_options(&self, options: &ContextBuildOptions) -> String {
        build_context_with_estimate(self.build_prompt_sections(&options.compression), options).0
    }

    pub fn estimate_prompt_context(
        &self,
        options: &ContextBuildOptions,
    ) -> ContextEstimateResponse {
        build_context_with_estimate(self.build_prompt_sections(&options.compression), options).1
    }

    pub fn build_prompt_sections(&self, mode: &ContextCompressionMode) -> Vec<ContextSection> {
        let mut sections = vec![ContextSection {
            id: "project",
            label: "Project",
            content: format!(
                "=== Project Context ===\nProject: {}\nContext mode: {}\n",
                self.project_path, mode
            ),
        }];

        if let Some(ref file) = self.active_file {
            sections.push(ContextSection {
                id: "active_file_path",
                label: "Active file path",
                content: format!("Active file: {}\n", file),
            });
        }
        if let Some(ref selection) = self.selection {
            sections.push(ContextSection {
                id: "selection",
                label: "Selection",
                content: format!("Selected code:\n```\n{}\n```\n", selection),
            });
        }
        if let Some(ref content) = self.active_file_content {
            let section_content = match mode {
                ContextCompressionMode::Full => {
                    format!("Current file content:\n```\n{}\n```\n", content)
                }
                ContextCompressionMode::Focused => {
                    format!(
                        "Current file excerpt:\n```\n{}\n```\n",
                        excerpt_text(content, 16_000)
                    )
                }
                ContextCompressionMode::Compact => {
                    format!(
                        "Current file summary: {} bytes, {} lines.\nCurrent file outline:\n```\n{}\n```\n",
                        content.len(),
                        content.lines().count(),
                        outline_text(content, 80)
                    )
                }
            };
            sections.push(ContextSection {
                id: "active_file_content",
                label: "Active file content",
                content: section_content,
            });
        }
        if !self.open_files.is_empty() {
            sections.push(ContextSection {
                id: "open_files",
                label: "Open files",
                content: format!("Open files: {:?}\n", self.open_files),
            });
        }
        if let Some(ref tree) = self.project_tree {
            if !tree.trim().is_empty() {
                sections.push(ContextSection {
                    id: "project_tree",
                    label: "Project tree",
                    content: format!("Project tree summary:\n```\n{}\n```\n", tree),
                });
            }
        }
        if let Some(ref diff) = self.git_diff {
            if !diff.trim().is_empty() {
                let section_content = match mode {
                    ContextCompressionMode::Full => {
                        format!("Git working tree diff:\n```diff\n{}\n```\n", diff)
                    }
                    ContextCompressionMode::Focused => {
                        format!(
                            "Git working tree diff excerpt:\n```diff\n{}\n```\n",
                            excerpt_text(diff, 12_000)
                        )
                    }
                    ContextCompressionMode::Compact => {
                        format!("Git diff summary: {}\n", summarize_diff(diff))
                    }
                };
                sections.push(ContextSection {
                    id: "git_diff",
                    label: "Git diff",
                    content: section_content,
                });
            }
        }

        sections
    }
}

pub fn estimated_input_tokens_from_budget(budget: &ContextBudget) -> Option<usize> {
    let max_context = budget.max_context_tokens?;
    let reserved = budget.reserved_output_tokens.unwrap_or(4096);
    Some(max_context.saturating_sub(reserved).saturating_sub(512))
}

fn build_context_with_estimate(
    sections: Vec<ContextSection>,
    options: &ContextBuildOptions,
) -> (String, ContextEstimateResponse) {
    let raw_chars = sections.iter().map(|section| section.content.len()).sum();
    let input_budget_tokens = options
        .budget
        .as_ref()
        .and_then(estimated_input_tokens_from_budget);
    let max_chars = input_budget_tokens.map(|tokens| tokens.saturating_mul(4));
    let mut remaining = max_chars.unwrap_or(usize::MAX);
    let mut output = String::new();
    let mut estimates = Vec::new();

    for section in sections {
        let raw_section_chars = section.content.len();
        let mut included = true;
        let mut trimmed = false;
        let mut excluded_reason = None;
        let mut content = section.content;

        if max_chars.is_some() {
            if remaining == 0 {
                included = false;
                content.clear();
                excluded_reason = Some("excluded because budget was exhausted".to_string());
            } else if content.len() > remaining {
                trimmed = true;
                content = excerpt_text(&content, remaining);
                remaining = 0;
                excluded_reason = Some("trimmed to fit input budget".to_string());
            } else {
                remaining = remaining.saturating_sub(content.len());
            }
        }

        if included {
            output.push_str(&content);
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }

        estimates.push(ContextEstimateSection {
            id: section.id.to_string(),
            label: section.label.to_string(),
            chars: raw_section_chars,
            estimated_tokens: estimate_tokens(raw_section_chars),
            included,
            trimmed,
            excluded_reason,
        });
    }

    let final_chars = output.len();
    let response = ContextEstimateResponse {
        sections: estimates,
        raw_chars,
        final_chars,
        estimated_tokens: estimate_tokens(final_chars),
        input_budget_tokens,
        trimmed: final_chars < raw_chars,
    };

    (output, response)
}

pub fn estimate_tokens(chars: usize) -> usize {
    chars.saturating_add(3) / 4
}

pub fn build_project_tree_summary(max_entries: usize, max_depth: usize) -> Result<String, String> {
    let root = crate::services::workspace::workspace_root()?;
    let mut entries = Vec::new();
    collect_tree_entries(&root, &root, 0, max_depth, max_entries, &mut entries)?;
    if entries.is_empty() {
        Ok("(empty workspace)".to_string())
    } else {
        Ok(entries.join("\n"))
    }
}

pub fn build_git_diff_summary(max_chars: usize) -> Result<String, String> {
    let root = crate::services::workspace::workspace_root()?;
    let repo = git2::Repository::discover(&root).map_err(|e| format!("Not a git repo: {}", e))?;
    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let tree = head.peel_to_tree().map_err(|e| format!("Tree: {}", e))?;
    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&tree), None)
        .map_err(|e| format!("Diff: {}", e))?;

    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_, _, line| {
        if output.len() >= max_chars {
            return false;
        }
        let origin = match line.origin() {
            '+' => '+',
            '-' => '-',
            ' ' => ' ',
            _ => ' ',
        };
        output.push(origin);
        output.push_str(&String::from_utf8_lossy(line.content()));
        true
    })
    .map_err(|e| format!("Diff print: {}", e))?;

    if output.len() >= max_chars {
        output.push_str("\n/* ... git diff truncated ... */\n");
    }
    Ok(output)
}

fn collect_tree_entries(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    entries: &mut Vec<String>,
) -> Result<(), String> {
    if depth > max_depth || entries.len() >= max_entries {
        return Ok(());
    }

    let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("Read dir {}: {}", dir.display(), e))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| !is_ignored_tree_path(path))
        .collect();
    children.sort_by(|a, b| {
        let a_is_dir = a.is_dir();
        let b_is_dir = b.is_dir();
        b_is_dir
            .cmp(&a_is_dir)
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });

    for path in children {
        if entries.len() >= max_entries {
            break;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let indent = "  ".repeat(depth);
        let suffix = if path.is_dir() { "/" } else { "" };
        entries.push(format!("{}{}{}", indent, relative.display(), suffix));
        if path.is_dir() {
            collect_tree_entries(root, &path, depth + 1, max_depth, max_entries, entries)?;
        }
    }

    Ok(())
}

fn is_ignored_tree_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | ".workbuddy"
            | ".DS_Store"
            | "Cargo.lock"
            | "package-lock.json"
    )
}

fn summarize_diff(diff: &str) -> String {
    let files = diff
        .lines()
        .filter(|line| line.starts_with("diff --git "))
        .count();
    let added = diff
        .lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .count();
    let removed = diff
        .lines()
        .filter(|line| line.starts_with('-') && !line.starts_with("---"))
        .count();
    format!("{} file(s), +{} -{} lines", files, added, removed)
}

fn excerpt_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let half = max_chars / 2;
    format!(
        "{}\n\n/* ... context truncated ... */\n\n{}",
        safe_prefix(text, half),
        safe_suffix(text, half)
    )
}

fn outline_text(text: &str, max_lines: usize) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("export interface ")
        {
            lines.push(line.to_string());
        }
        if lines.len() >= max_lines {
            break;
        }
    }
    if lines.is_empty() {
        text.lines().take(40).collect::<Vec<_>>().join("\n")
    } else {
        lines.join("\n")
    }
}

fn safe_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn safe_suffix(text: &str, max_bytes: usize) -> &str {
    let mut start = text.len().saturating_sub(max_bytes);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_context(content: &str) -> AgentContext {
        AgentContext {
            active_file: Some("src/app.ts".to_string()),
            active_file_content: Some(content.to_string()),
            selection: Some("const selected = true;".to_string()),
            open_files: vec!["src/app.ts".to_string()],
            project_path: "/workspace".to_string(),
            git_diff: None,
            project_tree: None,
        }
    }

    #[test]
    fn full_mode_includes_complete_content() {
        let ctx = sample_context("const a = 1;\nconst b = 2;");
        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Full);
        assert!(prompt.contains("Current file content"));
        assert!(prompt.contains("const b = 2;"));
    }

    #[test]
    fn compact_mode_uses_summary_and_outline() {
        let ctx = sample_context("const a = 1;\nfunction run() {}\nconst b = 2;");
        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Compact);
        assert!(prompt.contains("Current file summary"));
        assert!(prompt.contains("function run() {}"));
        assert!(!prompt.contains("Current file content"));
    }

    #[test]
    fn focused_mode_truncates_large_content() {
        let content = format!("start\n{}\nend", "x".repeat(20_000));
        let ctx = sample_context(&content);
        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Focused);
        assert!(prompt.contains("Current file excerpt"));
        assert!(prompt.contains("context truncated"));
    }

    #[test]
    fn prompt_includes_project_tree_and_git_diff_when_available() {
        let mut ctx = sample_context("const a = 1;");
        ctx.project_tree = Some("src/\n  src/app.ts".to_string());
        ctx.git_diff = Some("diff --git a/src/app.ts b/src/app.ts\n+const a = 2;\n".to_string());

        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Focused);

        assert!(prompt.contains("Project tree summary"));
        assert!(prompt.contains("src/app.ts"));
        assert!(prompt.contains("Git working tree diff excerpt"));
        assert!(prompt.contains("+const a = 2;"));
    }

    #[test]
    fn compact_mode_summarizes_git_diff() {
        let mut ctx = sample_context("const a = 1;");
        ctx.git_diff =
            Some("diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n-old\n+new\n".to_string());

        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Compact);

        assert!(prompt.contains("Git diff summary: 1 file(s), +1 -1 lines"));
        assert!(!prompt.contains("diff --git"));
    }

    #[test]
    fn context_source_options_can_disable_workspace_enrichment() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!(
            "agent-ide-context-source-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();

        let mut ctx = sample_context("const a = 1;");
        ctx.project_tree = None;
        ctx.git_diff = None;
        ctx.enrich_from_workspace_with_sources(&ContextSourceOptions {
            include_project_tree: false,
            include_git_diff: false,
        });

        assert!(ctx.project_tree.is_none());
        assert!(ctx.git_diff.is_none());
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn budget_options_trim_context_with_estimated_token_budget() {
        let ctx = sample_context(&"x".repeat(20_000));
        let prompt = ctx.to_prompt_context_with_options(&ContextBuildOptions::new(
            ContextCompressionMode::Full,
            Some(ContextBudget {
                max_context_tokens: Some(1200),
                reserved_output_tokens: Some(200),
            }),
        ));

        assert!(prompt.len() <= 4_000 + 80);
        assert!(prompt.contains("context truncated"));
    }

    #[test]
    fn context_estimate_reports_sections_and_trim_reasons() {
        let mut ctx = sample_context(&"x".repeat(20_000));
        ctx.project_tree = Some("src/\n  src/app.ts".to_string());
        ctx.git_diff = Some("diff --git a/src/app.ts b/src/app.ts\n+const a = 2;\n".to_string());

        let estimate = ctx.estimate_prompt_context(&ContextBuildOptions::new(
            ContextCompressionMode::Full,
            Some(ContextBudget {
                max_context_tokens: Some(1200),
                reserved_output_tokens: Some(200),
            }),
        ));

        assert!(estimate.raw_chars > estimate.final_chars);
        assert!(estimate.trimmed);
        assert!(estimate
            .sections
            .iter()
            .any(|section| section.id == "active_file_content"));
        assert!(estimate
            .sections
            .iter()
            .any(|section| section.trimmed || section.excluded_reason.is_some()));
    }
}
