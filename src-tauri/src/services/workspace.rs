use std::path::{Path, PathBuf};

pub fn config_dir() -> PathBuf {
    if let Ok(path) = std::env::var("AGENT_IDE_CONFIG_DIR") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".agent-ide")
}

pub fn save_workspace_path(path: &str) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Create config dir: {}", e))?;
    let file_path = dir.join("workspace.json");
    let json = serde_json::json!({ "path": path });
    let content =
        serde_json::to_string_pretty(&json).map_err(|e| format!("Serialize workspace: {}", e))?;
    std::fs::write(&file_path, content).map_err(|e| format!("Write workspace: {}", e))
}

pub fn load_workspace_path() -> Result<Option<String>, String> {
    let path = config_dir().join("workspace.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Parse workspace: {}", e))?;
    Ok(parsed
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

pub fn workspace_root() -> Result<PathBuf, String> {
    let configured = load_workspace_path()?;
    let root = match configured {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => std::env::current_dir().map_err(|e| format!("Current dir: {}", e))?,
    };
    root.canonicalize()
        .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e))
}

pub fn workspace_root_string() -> String {
    workspace_root()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn resolve_existing(path: &str) -> Result<PathBuf, String> {
    let candidate = normalize_candidate(path)?;
    let resolved = candidate
        .canonicalize()
        .map_err(|e| format!("Path does not exist or is not accessible: {}", e))?;
    ensure_within_workspace(&resolved)?;
    Ok(resolved)
}

pub fn resolve_for_write(path: &str) -> Result<PathBuf, String> {
    let candidate = normalize_candidate(path)?;
    if let Ok(existing) = candidate.canonicalize() {
        ensure_within_workspace(&existing)?;
        Ok(existing)
    } else {
        let ancestor = nearest_existing_ancestor(&candidate)
            .ok_or_else(|| format!("No existing parent for path: {}", path))?;
        let ancestor_resolved = ancestor
            .canonicalize()
            .map_err(|e| format!("Parent directory is not accessible: {}", e))?;
        ensure_within_workspace(&ancestor_resolved)?;
        Ok(candidate)
    }
}

pub fn ensure_within_workspace(path: &Path) -> Result<(), String> {
    let root = workspace_root()?;
    if path.starts_with(&root) {
        Ok(())
    } else {
        Err(format!(
            "Path is outside workspace: {}",
            path.to_string_lossy()
        ))
    }
}

fn normalize_candidate(path: &str) -> Result<PathBuf, String> {
    let raw = PathBuf::from(path);
    if raw.is_absolute() {
        Ok(raw)
    } else {
        Ok(workspace_root()?.join(raw))
    }
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(test)]
pub(crate) fn env_test_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("agent-ide-workspace-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn create_file(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
            path
        }

        fn outside_path(&self, relative: &str) -> PathBuf {
            let base = self
                .root
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("outside");
            let path = base.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            path
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(
                self.root
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.root.clone()),
            );
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    #[test]
    fn resolve_existing_allows_path_inside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        env.create_file("src/main.ts", "export const ok = true;");

        let resolved = resolve_existing("src/main.ts").unwrap();

        assert!(resolved.starts_with(&env.root));
        assert!(resolved.ends_with(Path::new("src").join("main.ts")));
    }

    #[test]
    fn resolve_existing_rejects_path_outside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        let outside = env.outside_path("secret.txt");
        std::fs::write(&outside, "nope").unwrap();

        let err = resolve_existing(outside.to_string_lossy().as_ref()).unwrap_err();

        assert!(err.contains("outside workspace"));
    }

    #[test]
    fn resolve_for_write_allows_new_path_inside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();

        let resolved = resolve_for_write("nested/new/file.ts").unwrap();

        assert!(resolved.starts_with(&env.root));
        assert!(resolved.ends_with(Path::new("nested").join("new").join("file.ts")));
    }

    #[test]
    fn resolve_for_write_rejects_new_path_outside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        let outside = env.outside_path("nested/new/file.ts");

        let err = resolve_for_write(outside.to_string_lossy().as_ref()).unwrap_err();

        assert!(err.contains("outside workspace"));
    }

    #[test]
    fn resolve_existing_rejects_relative_traversal_outside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        let outside = env.outside_path("secret.txt");
        std::fs::write(&outside, "nope").unwrap();

        let relative = Path::new("..")
            .join("..")
            .join("outside")
            .join("secret.txt")
            .to_string_lossy()
            .to_string();
        let err = resolve_existing(&relative).unwrap_err();

        assert!(err.contains("outside workspace"));
    }
}
