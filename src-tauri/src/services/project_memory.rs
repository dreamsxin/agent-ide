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
        let temp =
            std::env::temp_dir().join(format!("agent-ide-project-memory-test-{}", Uuid::new_v4()));
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

    /// **这个仓库自己的** `AGENTS.md` 必须装得下，而且要留出余量。
    ///
    /// 截断机制本身上面已经测过了 —— 但那只证明"超了会被切"，不证明"我们的那份没超"。
    /// 而这份文件被注入每一次 Agent 运行，被切掉的是**尾部**，也就是"决定写到哪里"那一节：
    /// 没有任何报错，只是从此每次运行都少看到几条规则。这条测试把那次静默变成一次红色。
    ///
    /// 留 500 字节余量而不是贴着 8 000：贴着上限的话，下一个人加一行就越界，而越界的症状
    /// 是看不见的。`AGENTS.md` 自己写的就是"keep it under ~7 500"，这里把那句话变成可执行的。
    #[test]
    fn this_repos_project_memory_fits_with_headroom() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(PROJECT_MEMORY_FILE);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读不到 {}: {}", path.display(), error));
        let bytes = content.trim().len();

        // 硬上限：超过就会被真的切掉
        assert!(
            bytes <= MAX_PROJECT_MEMORY_CHARS,
            "AGENTS.md 有 {} 字节，超过了 {} 的注入上限，尾部会被静默切掉",
            bytes,
            MAX_PROJECT_MEMORY_CHARS
        );
        // 余量：越界的症状看不见，所以在还看得见的时候就报
        let recommended = MAX_PROJECT_MEMORY_CHARS - 500;
        assert!(
            bytes <= recommended,
            "AGENTS.md 有 {} 字节，已经超过建议的 {}；该精简，而不是继续往上加",
            bytes,
            recommended
        );
    }
}
