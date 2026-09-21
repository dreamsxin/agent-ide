use crate::services::workspace;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tauri::AppHandle;
use tauri::Emitter;

#[derive(Debug, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

// ====== 文件监听状态 ======
pub struct FileWatcherState {
    pub watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    pub running: Arc<Mutex<bool>>,
}

impl FileWatcherState {
    pub fn new() -> Self {
        Self {
            watcher: Arc::new(Mutex::new(None)),
            running: Arc::new(Mutex::new(false)),
        }
    }
}

// ====== 基础读取 ======

/// 读取文件内容
#[tauri::command]
pub fn read_file_content(path: String) -> Result<String, String> {
    let path = workspace::resolve_existing(&path)?;
    fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

/// 写入文件内容（自动创建父目录）
#[tauri::command]
pub fn write_file_content(path: String, content: String) -> Result<(), String> {
    let path = workspace::resolve_for_write(&path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }
    fs::write(&path, &content).map_err(|e| format!("Failed to write file: {}", e))
}

/// 列出目录内容
#[tauri::command]
pub fn list_directory(path: String) -> Result<Vec<FileEntry>, String> {
    let path = workspace::resolve_existing(&path)?;
    let dir = fs::read_dir(&path).map_err(|e| format!("Failed to read dir: {}", e))?;

    let mut entries = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to get metadata: {}", e))?;

        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

// ====== CRUD 操作 ======

/// 删除文件或递归删除目录
#[tauri::command]
pub fn delete_path(path: String) -> Result<(), String> {
    let resolved = workspace::resolve_existing(&path)?;
    let p = resolved.as_path();
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if p.is_dir() {
        fs::remove_dir_all(&resolved).map_err(|e| format!("Failed to delete directory: {}", e))
    } else {
        fs::remove_file(&resolved).map_err(|e| format!("Failed to delete file: {}", e))
    }
}

/// 创建文件（可选初始内容）
#[tauri::command]
pub fn create_file(path: String, content: Option<String>) -> Result<(), String> {
    let resolved = workspace::resolve_for_write(&path)?;
    if resolved.exists() {
        return Err(format!("File already exists: {}", path));
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    fs::write(&resolved, content.unwrap_or_default())
        .map_err(|e| format!("Failed to create file: {}", e))
}

/// 创建目录（递归）
#[tauri::command]
pub fn create_directory(path: String) -> Result<(), String> {
    let resolved = workspace::resolve_for_write(&path)?;
    fs::create_dir_all(&resolved).map_err(|e| format!("Failed to create directory: {}", e))
}

/// 重命名/移动文件或目录
#[tauri::command]
pub fn rename_path(old_path: String, new_path: String) -> Result<(), String> {
    let old_resolved = workspace::resolve_existing(&old_path)?;
    let new_resolved = workspace::resolve_for_write(&new_path)?;
    if !old_resolved.exists() {
        return Err(format!("Source path does not exist: {}", old_path));
    }
    if new_resolved.exists() {
        return Err(format!("Target path already exists: {}", new_path));
    }
    // 和 `copy_path` 同一条守卫，理由略有不同：`fs::rename` 本身会拒绝把目录搬进自己
    // 内部（Linux 上 EINVAL），但报出来的是一句裸的操作系统错误，看不出问题在哪。
    // 这里提前拦住，给一句说得清的话，也让这两个命令的行为对得上。
    // `starts_with` 按路径分量比，所以 `/w/ab` 不会被误判成在 `/w/a` 里面。
    if old_resolved.is_dir() && new_resolved.starts_with(&old_resolved) {
        return Err(format!(
            "Cannot move a directory into itself: {} -> {}",
            old_path, new_path
        ));
    }
    // 确保目标父目录存在
    if let Some(parent) = new_resolved.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    fs::rename(&old_resolved, &new_resolved).map_err(|e| format!("Failed to rename: {}", e))
}

/// Open the system file manager at the selected file or directory.
#[tauri::command]
pub fn reveal_in_file_explorer(path: String) -> Result<(), String> {
    let resolved = workspace::resolve_existing(&path)?;
    reveal_path(&resolved)
}

// 每个平台分支都必须显式 return：其他平台的分支在当前 cfg 下被裁剪掉，
// clippy 因此把最后一个分支的 return 当成多余的。
#[allow(clippy::needless_return)]
fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if path.is_dir() {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed to open File Explorer: {}", e))?;
        } else {
            Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .map_err(|e| format!("Failed to reveal in File Explorer: {}", e))?;
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        if path.is_dir() {
            Command::new("open")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed to open Finder: {}", e))?;
        } else {
            Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        Err("Reveal in file explorer is not supported on this platform.".to_string())
    }
}

// ====== 增强文件操作 ======

/// 文件/目录元数据
#[derive(Debug, Serialize)]
pub struct FileMetadata {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64, // UNIX timestamp (seconds)
    pub readonly: bool,
}

/// 获取文件/目录元数据
#[tauri::command]
pub fn get_file_metadata(path: String) -> Result<FileMetadata, String> {
    let resolved = workspace::resolve_existing(&path)?;
    let p = resolved.as_path();
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let metadata =
        fs::metadata(&resolved).map_err(|e| format!("Failed to read metadata: {}", e))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let readonly = metadata.permissions().readonly();
    Ok(FileMetadata {
        name: p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone()),
        path: resolved.to_string_lossy().to_string(),
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        modified,
        readonly,
    })
}

/// 复制文件或递归复制目录
#[tauri::command]
pub fn copy_path(src: String, dest: String) -> Result<(), String> {
    let src_resolved = workspace::resolve_existing(&src)?;
    let src_path = src_resolved.as_path();
    if !src_path.exists() {
        return Err(format!("Source does not exist: {}", src));
    }
    let dest_resolved = workspace::resolve_for_write(&dest)?;
    let dest_path = dest_resolved.as_path();
    // 目录不能拷进自己内部。`copy_dir_recursive` 先建 dest 再遍历 src，dest 落在 src
    // 里就意味着遍历会撞上自己刚创建的那个目录 —— 会不会一路递归下去取决于平台
    // `read_dir` 的语义，所以结果是"不可预测"，不是"慢"。这条路径两次点击就能走到：
    // Explorer 的粘贴目标取的是被右击的那个目录本身，复制一个文件夹再右击它自己
    // 粘贴就落在这里。
    //
    // 放在 `dest_path.exists()` 之前：dest 恰好等于 src 时那条检查也会拦住，但报的是
    // "目标已存在"，把问题指向了错的地方。`starts_with` 是按路径分量比的，所以
    // `/w/ab` 不会被误判成在 `/w/a` 里面。
    if src_path.is_dir() && dest_path.starts_with(src_path) {
        return Err(format!(
            "Cannot copy a directory into itself: {} -> {}",
            src, dest
        ));
    }
    if dest_path.exists() {
        return Err(format!("Destination already exists: {}", dest));
    }
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    if src_path.is_dir() {
        copy_dir_recursive(src_path, dest_path)?;
    } else {
        fs::copy(src_path, dest_path).map_err(|e| format!("Failed to copy file: {}", e))?;
    }
    Ok(())
}

/// 递归复制目录
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("Failed to create dest dir: {}", e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read src dir: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_entry = entry.path();
        let dest_entry = dest.join(entry.file_name());
        if src_entry.is_dir() {
            copy_dir_recursive(&src_entry, &dest_entry)?;
        } else {
            fs::copy(&src_entry, &dest_entry)
                .map_err(|e| format!("Failed to copy {}: {}", src_entry.display(), e))?;
        }
    }
    Ok(())
}

/// 按 glob 模式搜索文件
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[tauri::command]
pub fn search_files(
    root: String,
    pattern: String,
    max_depth: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let root_resolved = workspace::resolve_existing(&root)?;
    let root_path = root_resolved.as_path();
    if !root_path.is_dir() {
        return Err(format!("Not a directory: {}", root));
    }
    let max_depth = max_depth.unwrap_or(20);
    let mut results = Vec::new();
    search_recursive(root_path, &pattern, 0, max_depth, &mut results)?;
    results.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(results)
}

fn search_recursive(
    dir: &Path,
    pattern: &str,
    depth: u32,
    max_depth: u32,
    results: &mut Vec<SearchResult>,
) -> Result<(), String> {
    if depth > max_depth {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to get metadata: {}", e))?;

        // Simple glob match
        if glob_match(&name, pattern) {
            results.push(SearchResult {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            });
        }
        if metadata.is_dir() {
            search_recursive(&path, pattern, depth + 1, max_depth, results)?;
        }
    }
    Ok(())
}

/// Simple glob matching supporting * and ?
fn glob_match(name: &str, pattern: &str) -> bool {
    let name = name.to_lowercase();
    let pattern = pattern.to_lowercase();
    let name_bytes = name.as_bytes();
    let pat_bytes = pattern.as_bytes();
    let n = name_bytes.len();
    let m = pat_bytes.len();

    // DP table
    let mut dp = vec![vec![false; m + 1]; n + 1];
    dp[0][0] = true;
    for j in 1..=m {
        if pat_bytes[j - 1] == b'*' {
            dp[0][j] = dp[0][j - 1];
        }
    }
    for i in 1..=n {
        for j in 1..=m {
            if pat_bytes[j - 1] == b'*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if pat_bytes[j - 1] == b'?' || pat_bytes[j - 1] == name_bytes[i - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[n][m]
}

// ====== 文件监听 ======

/// 开始监听项目目录的文件变更
#[tauri::command]
pub fn watch_start(
    app: AppHandle,
    state: tauri::State<'_, FileWatcherState>,
) -> Result<(), String> {
    use notify::{Config, EventKind, RecursiveMode, Watcher};
    use std::time::Duration;

    let running = state.running.clone();
    {
        let mut r = running.lock().map_err(|e| e.to_string())?;
        if *r {
            return Ok(()); // 已经在监听
        }
        *r = true;
    }

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .configure(Config::default().with_poll_interval(Duration::from_secs(2)))
        .map_err(|e| format!("Failed to configure watcher: {}", e))?;

    let cwd = workspace::workspace_root()?;
    watcher
        .watch(&cwd, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to start watching: {}", e))?;

    // 存储 watcher（保持存活）
    {
        let mut w = state
            .watcher
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        *w = Some(watcher);
    }

    // 后台线程：接收事件 → emit 到前端
    let running_clone = running.clone();
    std::thread::spawn(move || {
        for event_res in rx {
            match event_res {
                Ok(event) => {
                    // 忽略纯 Access 事件，减少噪声
                    if matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    let paths: Vec<String> = event
                        .paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    let _ = app.emit(
                        "file-changed",
                        serde_json::json!({
                            "kind": format!("{:?}", event.kind),
                            "paths": paths,
                        }),
                    );
                }
                Err(_) => break,
            }
        }
        // watcher dropped here → stop
        if let Ok(mut r) = running_clone.lock() {
            *r = false;
        }
    });

    Ok(())
}

/// 停止文件监听
#[tauri::command]
pub fn watch_stop(state: tauri::State<'_, FileWatcherState>) -> Result<(), String> {
    let mut w = state
        .watcher
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    *w = None; // drop watcher → stops the thread
    let mut r = state
        .running
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    *r = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!("agent-ide-fs-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }

        fn at(&self, relative: &str) -> String {
            self.root.join(relative).to_string_lossy().to_string()
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(self.config_dir.parent().unwrap());
        }
    }

    /// 移动目录进自己内部：`fs::rename` 自己也会拒，但报的是一句裸的系统错误。
    /// 断言落在"说得清"和"什么都没动"上。
    #[test]
    fn moving_a_directory_into_itself_is_refused_with_a_clear_message() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/b/f.txt", "x\n");

        let error = rename_path(env.at("a"), env.at("a/b/a")).unwrap_err();
        assert!(error.contains("into itself"), "{}", error);
        assert!(env.root.join("a/b/f.txt").is_file());
    }

    /// 守卫不能拦过头：`ab` 只是名字以 `a` 开头，并不在 `a` 里面。
    #[test]
    fn moving_a_directory_next_to_a_similarly_named_sibling_works() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/f.txt", "x\n");
        std::fs::create_dir_all(env.root.join("ab")).unwrap();

        rename_path(env.at("a"), env.at("ab/a")).unwrap();
        assert!(env.root.join("ab/a/f.txt").is_file());
        assert!(!env.root.join("a").exists());
    }

    /// 把一个目录粘贴到它自己里面：`copy_dir_recursive` 会先建好目标目录，再去遍历
    /// 源目录，于是遍历有机会撞上刚创建的那个目标。断言落在"什么都没发生"上，而不是
    /// 具体错误文案。
    #[test]
    fn copying_a_directory_into_itself_is_refused() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/f.txt", "x\n");

        let error = copy_path(env.at("a"), env.at("a/a Copy")).unwrap_err();
        assert!(error.contains("into itself"), "{}", error);
        assert!(!env.root.join("a/a Copy").exists());
    }

    /// 粘贴到自己的子目录同样不行 —— 这是 Explorer 里更容易点到的那一种：复制父目录，
    /// 右击它下面的某个子目录粘贴。
    #[test]
    fn copying_a_directory_into_its_own_descendant_is_refused() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/b/f.txt", "x\n");

        let error = copy_path(env.at("a"), env.at("a/b/a Copy")).unwrap_err();
        assert!(error.contains("into itself"), "{}", error);
        assert!(!env.root.join("a/b/a Copy").exists());
    }

    /// 守卫不能拦过头。第二个断言是关键：`ab` 只是名字以 `a` 开头，并不在 `a` 里面 ——
    /// 用字符串前缀比而不是路径分量比，就会把它误判成"拷进自己内部"。
    #[test]
    fn copying_a_directory_elsewhere_still_works() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/f.txt", "x\n");
        std::fs::create_dir_all(env.root.join("ab")).unwrap();

        copy_path(env.at("a"), env.at("c")).unwrap();
        assert_eq!(
            std::fs::read_to_string(env.root.join("c/f.txt")).unwrap(),
            "x\n"
        );

        copy_path(env.at("a"), env.at("ab/a Copy")).unwrap();
        assert!(env.root.join("ab/a Copy/f.txt").is_file());
    }

    /// 文件不受这条守卫影响：只有目录才有"拷进自己内部"这回事。
    #[test]
    fn copying_a_file_next_to_itself_still_works() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("a/f.txt", "x\n");

        copy_path(env.at("a/f.txt"), env.at("a/f Copy.txt")).unwrap();
        assert!(env.root.join("a/f Copy.txt").is_file());
    }
}
