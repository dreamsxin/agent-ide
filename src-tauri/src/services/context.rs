use serde::{Deserialize, Serialize};

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
}

impl AgentContext {
    pub fn new(project_path: &str) -> Self {
        Self {
            active_file: None,
            active_file_content: None,
            selection: None,
            open_files: Vec::new(),
            project_path: project_path.to_string(),
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

        ctx
    }
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
}
