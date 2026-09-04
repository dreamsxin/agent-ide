use std::path::PathBuf;

pub const PROJECT_MEMORY_FILE: &str = "AGENTS.md";
pub const MAX_PROJECT_MEMORY_CHARS: usize = 8_000;

pub fn project_memory_path() -> Result<PathBuf, String> {
    let root = crate::services::workspace::workspace_root()?;
    Ok(root.join(PROJECT_MEMORY_FILE))
}

/// Load the workspace-root `AGENTS.md` project memory file.
///
/// Returns `Ok(None)` when the file does not exist. Content is trimmed and
/// bounded so it stays predictable inside context budget packing.
pub fn load_project_memory() -> Result<Option<String>, String> {
    let path = project_memory_path()?;
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Read {}: {}", path.display(), err)),
    };
    Ok(Some(bound_project_memory(&content)))
}

pub fn bound_project_memory(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.len() <= MAX_PROJECT_MEMORY_CHARS {
        return trimmed.to_string();
    }
    format!(
        "{}\n\n/* ... project memory truncated ... */",
        safe_prefix(trimmed, MAX_PROJECT_MEMORY_CHARS)
    )
}

fn safe_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn bound_project_memory_keeps_small_content() {
        let bounded = bound_project_memory("# Agent rules\n\nAlways run cargo test.\n");

        assert_eq!(bounded, "# Agent rules\n\nAlways run cargo test.");
    }

    #[test]
    fn bound_project_memory_truncates_oversized_content() {
        let content = format!("# Rules\n\n{}", "x".repeat(MAX_PROJECT_MEMORY_CHARS + 100));

        let bounded = bound_project_memory(&content);

        assert!(bounded.contains("project memory truncated"));
        assert!(bounded.len() <= MAX_PROJECT_MEMORY_CHARS + 80);
    }

    #[test]
    fn bound_project_memory_respects_char_boundaries() {
        let content = "中文".repeat((MAX_PROJECT_MEMORY_CHARS / 6) + 10);

        let bounded = bound_project_memory(&content);

        assert!(bounded.contains("project memory truncated"));
    }

    #[test]
    fn load_project_memory_reads_workspace_agents_md() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!(
            "agent-ide-project-memory-test-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();
        std::fs::write(
            temp.join(PROJECT_MEMORY_FILE),
            "# Rules\n\nAlways run cargo test.\n",
        )
        .unwrap();

        let memory = load_project_memory().unwrap().expect("project memory");

        assert!(memory.contains("Always run cargo test."));

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn load_project_memory_returns_none_when_missing() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!(
            "agent-ide-project-memory-missing-test-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();

        assert!(load_project_memory().unwrap().is_none());

        let _ = std::fs::remove_dir_all(temp);
    }
}
