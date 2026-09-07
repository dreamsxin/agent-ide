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
    let normalized = shell_compatible_path(PathBuf::from(path));
    let json = serde_json::json!({ "path": normalized.to_string_lossy() });
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
        .map(shell_compatible_path)
        .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e))
}

pub fn workspace_root_string() -> String {
    workspace_root()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn shell_compatible_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{}", rest));
        }
        if let Some(rest) = text.strip_prefix("\\\\?\\") {
            return PathBuf::from(rest);
        }
    }

    path
}

pub fn resolve_existing(path: &str) -> Result<PathBuf, String> {
    let candidate = normalize_candidate(path)?;
    let resolved = candidate
        .canonicalize()
        .map(shell_compatible_path)
        .map_err(|e| format!("Path does not exist or is not accessible: {}", e))?;
    ensure_within_workspace(&resolved)?;
    Ok(resolved)
}

pub fn resolve_for_write(path: &str) -> Result<PathBuf, String> {
    let candidate = normalize_candidate(path)?;
    if let Ok(existing) = candidate.canonicalize().map(shell_compatible_path) {
        ensure_within_workspace(&existing)?;
        Ok(existing)
    } else {
        let ancestor = nearest_existing_ancestor(&candidate)
            .ok_or_else(|| format!("No existing parent for path: {}", path))?;
        let ancestor_resolved = ancestor
            .canonicalize()
            .map(shell_compatible_path)
            .map_err(|e| format!("Parent directory is not accessible: {}", e))?;
        ensure_within_workspace(&ancestor_resolved)?;
        Ok(shell_compatible_path(candidate))
    }
}

/// Agent 生成的写入：先过工作区边界，再过拒绝清单。
///
/// `resolve_for_write` 只保证"在工作区内"，但工作区内本身就有几类"写进去等于
/// 拿到执行权或碰到密钥"的路径：
/// - `.git/`：写 `.git/hooks/pre-commit` 等于在下一次 commit 时获得任意代码执行，
///   写 `.git/config` 可以改 remote 或插入 `core.fsmonitor`。
/// - 凭据文件（`.env*`、`*.pem`、`id_rsa` 等）：改写它们造成的泄漏或覆盖不可逆。
/// - `.agent-ide/`、`node_modules/`：工具自身的状态和依赖树，不是源码。
///
/// 用户在文件浏览器里手动编辑这些文件仍然允许 —— 这条规则只约束 Agent 产出的
/// diff，所以它是独立函数，而不是塞进 `resolve_for_write`。
pub fn resolve_for_agent_write(path: &str) -> Result<PathBuf, String> {
    let resolved = resolve_for_write(path)?;
    // 只检查工作区内部的相对路径：工作区自己可能就放在
    // `D:\work\node_modules\demo` 这种目录下，用绝对路径判断会把一切都拒掉
    let root = workspace_root()?;
    let relative = resolved.strip_prefix(&root).unwrap_or(&resolved);
    if let Some(reason) = agent_write_denial(relative) {
        return Err(reason);
    }
    Ok(resolved)
}

/// 命中拒绝清单时返回具体原因，否则返回 None
fn agent_write_denial(path: &Path) -> Option<String> {
    /// 目录名：出现在路径任意一层都拒绝（子模块的 `.git`、嵌套的 node_modules）
    const DENIED_DIRS: [&str; 3] = [".git", ".agent-ide", "node_modules"];

    // 大小写不敏感比较：Windows 上 `.GIT/hooks/pre-commit` 指向同一个文件
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect();

    let (file_name, dirs) = components.split_last()?;

    if let Some(dir) = dirs.iter().find(|dir| DENIED_DIRS.contains(&dir.as_str())) {
        return Some(format!(
            "Agent writes into {}/ are not allowed: {}",
            dir,
            path.display()
        ));
    }
    if DENIED_DIRS.contains(&file_name.as_str()) {
        return Some(format!(
            "Agent writes into {}/ are not allowed: {}",
            file_name,
            path.display()
        ));
    }

    // `.env` 本身以及 `.env.local` / `.env.production` 这类变体
    if is_credential_file_name(file_name) {
        return Some(format!(
            "Agent writes to credential file {} are not allowed",
            file_name
        ));
    }

    None
}

/// 凭据类文件名（传入的必须已小写）
fn is_credential_file_name(file_name: &str) -> bool {
    /// 文件名整体匹配
    const DENIED_FILES: [&str; 5] = [".env", ".npmrc", ".netrc", "id_rsa", "id_ed25519"];
    /// 扩展名匹配
    const DENIED_EXTENSIONS: [&str; 4] = ["pem", "key", "p12", "pfx"];

    if DENIED_FILES.contains(&file_name) || file_name.starts_with(".env.") {
        return true;
    }
    file_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| DENIED_EXTENSIONS.contains(&extension))
}

/// 这个路径是否是凭据类文件，因而不该被送进发给模型的上下文。
///
/// 和写入拒绝清单共用同一份文件名规则，但刻意不含 `.git` / `node_modules`：
/// 那两个是完整性和噪音问题，不是外泄问题。这里只接受路径字符串（可以是
/// 相对路径或纯文件名），因为上下文里的条目不一定落在磁盘上。
pub fn is_credential_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let file_name = normalized
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(&normalized)
        .to_lowercase();
    is_credential_file_name(&file_name)
}

pub fn ensure_within_workspace(path: &Path) -> Result<(), String> {
    let root = workspace_root()?;
    let normalized_path = shell_compatible_path(path.to_path_buf());
    if normalized_path.starts_with(&root) {
        Ok(())
    } else {
        Err(format!(
            "Path is outside workspace: {}",
            normalized_path.to_string_lossy()
        ))
    }
}

fn normalize_candidate(path: &str) -> Result<PathBuf, String> {
    let raw = shell_compatible_path(PathBuf::from(path));
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
            let root = shell_compatible_path(root.canonicalize().unwrap());
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

    // `\\?\` 是 Windows 独有的 verbatim 前缀。在 Unix 上它只是普通文件名字符，
    // 拼出来的路径必然不存在，所以这两个断言只在 Windows 上成立。
    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_existing_allows_windows_verbatim_path_inside_workspace() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        let file = env.create_file("src/main.ts", "export const ok = true;");
        let verbatim = format!("\\\\?\\{}", file.to_string_lossy());

        let resolved = resolve_existing(&verbatim).unwrap();

        assert!(resolved.starts_with(&env.root));
        assert!(resolved.ends_with(Path::new("src").join("main.ts")));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn ensure_within_workspace_allows_windows_verbatim_workspace_root() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        let verbatim = PathBuf::from(format!("\\\\?\\{}", env.root.to_string_lossy()));

        ensure_within_workspace(&verbatim).unwrap();
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

    /// 写 `.git/hooks/pre-commit` 等于在下一次 commit 时拿到任意代码执行，
    /// 而 `resolve_for_write` 只检查"在工作区内"，是允许的。
    #[test]
    fn agent_writes_into_dot_git_are_denied() {
        let _guard = env_test_guard();
        let env = TestEnv::new();
        env.create_file(".git/hooks/pre-commit", "#!/bin/sh\n");

        let err = resolve_for_agent_write(".git/hooks/pre-commit").unwrap_err();
        assert!(err.contains(".git/"), "{}", err);

        // 用户自己在文件浏览器里编辑仍然允许 —— 拒绝清单只约束 Agent
        assert!(resolve_for_write(".git/hooks/pre-commit").is_ok());
    }

    #[test]
    fn agent_writes_to_credential_files_are_denied() {
        let _guard = env_test_guard();
        let _env = TestEnv::new();

        for path in [
            ".env",
            ".env.production",
            "config/id_rsa",
            "certs/server.pem",
            "secrets/api.key",
            "node_modules/pkg/index.js",
            ".agent-ide/state.json",
        ] {
            let err = resolve_for_agent_write(path)
                .expect_err(&format!("expected {} to be denied", path));
            assert!(err.contains("not allowed"), "{}: {}", path, err);
        }
    }

    #[test]
    fn agent_writes_to_ordinary_source_files_are_allowed() {
        let _guard = env_test_guard();
        let env = TestEnv::new();

        let resolved = resolve_for_agent_write("src/env/loader.ts").unwrap();

        assert!(resolved.starts_with(&env.root));
        // 目录名叫 env、文件名含 key 都不该误伤
        assert!(resolve_for_agent_write("src/keyboard.ts").is_ok());
        assert!(resolve_for_agent_write("src/environment.ts").is_ok());
    }

    /// 出网方向复用同一份凭据文件规则，但不含 `.git` / `node_modules`：
    /// 后两者是完整性和噪音问题，不是泄漏问题。
    #[test]
    fn credential_paths_are_recognized_for_context_exclusion() {
        for path in [
            ".env",
            ".env.production",
            "config/.ENV.local",
            "keys/id_rsa",
            "certs/server.pem",
            "a\\b\\api.key",
            ".npmrc",
        ] {
            assert!(is_credential_path(path), "{} should be credential", path);
        }

        for path in [
            "src/env/loader.ts",
            "src/keyboard.ts",
            "src/environment.ts",
            ".git/config",
            "node_modules/pkg/index.js",
            "README.md",
        ] {
            assert!(
                !is_credential_path(path),
                "{} should not be credential",
                path
            );
        }
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

    #[test]
    fn shell_compatible_path_strips_windows_verbatim_disk_prefix() {
        let input = PathBuf::from("\\\\?\\D:\\work\\project");
        let output = shell_compatible_path(input);

        if cfg!(target_os = "windows") {
            assert_eq!(output, PathBuf::from("D:\\work\\project"));
        } else {
            assert_eq!(output, PathBuf::from("\\\\?\\D:\\work\\project"));
        }
    }
}
