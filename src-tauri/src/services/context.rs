use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompressionMode {
    Full,
    Focused,
    Compact,
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
        if self.project_tree.is_none() {
            self.project_tree = build_project_tree_summary(160, 4).ok();
        }
        if self.git_diff.is_none() {
            self.git_diff = build_git_diff_summary(24_000).ok();
        }
    }

    pub fn to_prompt_context(&self) -> String {
        self.to_prompt_context_with_mode(&ContextCompressionMode::Full)
    }

    pub fn to_prompt_context_with_mode(&self, mode: &ContextCompressionMode) -> String {
        let mut ctx = String::new();
        ctx.push_str("=== Project Context ===\n");
        ctx.push_str(&format!("Project: {}\n", self.project_path));
        ctx.push_str(&format!("Context mode: {}\n", mode));

        if let Some(ref file) = self.active_file {
            ctx.push_str(&format!("Active file: {}\n", file));
        }
        if let Some(ref selection) = self.selection {
            ctx.push_str(&format!("Selected code:\n```\n{}\n```\n", selection));
        }
        if let Some(ref content) = self.active_file_content {
            match mode {
                ContextCompressionMode::Full => {
                    ctx.push_str(&format!("Current file content:\n```\n{}\n```\n", content));
                }
                ContextCompressionMode::Focused => {
                    ctx.push_str(&format!(
                        "Current file excerpt:\n```\n{}\n```\n",
                        excerpt_text(content, 16_000)
                    ));
                }
                ContextCompressionMode::Compact => {
                    ctx.push_str(&format!(
                        "Current file summary: {} bytes, {} lines.\n",
                        content.len(),
                        content.lines().count()
                    ));
                    ctx.push_str(&format!(
                        "Current file outline:\n```\n{}\n```\n",
                        outline_text(content, 80)
                    ));
                }
            }
        }
        if !self.open_files.is_empty() {
            ctx.push_str(&format!("Open files: {:?}\n", self.open_files));
        }
        if let Some(ref tree) = self.project_tree {
            if !tree.trim().is_empty() {
                ctx.push_str(&format!("Project tree summary:\n```\n{}\n```\n", tree));
            }
        }
        if let Some(ref diff) = self.git_diff {
            if !diff.trim().is_empty() {
                match mode {
                    ContextCompressionMode::Full => {
                        ctx.push_str(&format!("Git working tree diff:\n```diff\n{}\n```\n", diff));
                    }
                    ContextCompressionMode::Focused => {
                        ctx.push_str(&format!(
                            "Git working tree diff excerpt:\n```diff\n{}\n```\n",
                            excerpt_text(diff, 12_000)
                        ));
                    }
                    ContextCompressionMode::Compact => {
                        ctx.push_str(&format!("Git diff summary: {}\n", summarize_diff(diff)));
                    }
                }
            }
        }

        ctx
    }
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
        ctx.git_diff = Some(
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n-old\n+new\n".to_string(),
        );

        let prompt = ctx.to_prompt_context_with_mode(&ContextCompressionMode::Compact);

        assert!(prompt.contains("Git diff summary: 1 file(s), +1 -1 lines"));
        assert!(!prompt.contains("diff --git"));
    }
}
