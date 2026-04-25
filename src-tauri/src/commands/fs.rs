use serde::Serialize;
use std::fs;
use std::path::Path;
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
    fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

/// 写入文件内容（自动创建父目录）
#[tauri::command]
pub fn write_file_content(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }
    fs::write(&path, &content).map_err(|e| format!("Failed to write file: {}", e))
}

/// 列出目录内容
#[tauri::command]
pub fn list_directory(path: String) -> Result<Vec<FileEntry>, String> {
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

/// 检查文件是否存在
#[tauri::command]
pub fn file_exists(path: String) -> Result<bool, String> {
    Ok(Path::new(&path).exists())
}

// ====== CRUD 操作 ======

/// 删除文件或递归删除目录
#[tauri::command]
pub fn delete_path(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if p.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| format!("Failed to delete directory: {}", e))
    } else {
        fs::remove_file(&path).map_err(|e| format!("Failed to delete file: {}", e))
    }
}

/// 创建文件（可选初始内容）
#[tauri::command]
pub fn create_file(path: String, content: Option<String>) -> Result<(), String> {
    if Path::new(&path).exists() {
        return Err(format!("File already exists: {}", path));
    }
    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    fs::write(&path, content.unwrap_or_default())
        .map_err(|e| format!("Failed to create file: {}", e))
}

/// 创建目录（递归）
#[tauri::command]
pub fn create_directory(path: String) -> Result<(), String> {
    fs::create_dir_all(&path).map_err(|e| format!("Failed to create directory: {}", e))
}

/// 重命名/移动文件或目录
#[tauri::command]
pub fn rename_path(old_path: String, new_path: String) -> Result<(), String> {
    if !Path::new(&old_path).exists() {
        return Err(format!("Source path does not exist: {}", old_path));
    }
    if Path::new(&new_path).exists() {
        return Err(format!("Target path already exists: {}", new_path));
    }
    // 确保目标父目录存在
    if let Some(parent) = Path::new(&new_path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    fs::rename(&old_path, &new_path)
        .map_err(|e| format!("Failed to rename: {}", e))
}

// ====== 增强文件操作 ======

/// 文件/目录元数据
#[derive(Debug, Serialize)]
pub struct FileMetadata {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,  // UNIX timestamp (seconds)
    pub readonly: bool,
}

/// 获取文件/目录元数据
#[tauri::command]
pub fn get_file_metadata(path: String) -> Result<FileMetadata, String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let metadata = fs::metadata(&path).map_err(|e| format!("Failed to read metadata: {}", e))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let readonly = metadata.permissions().readonly();
    Ok(FileMetadata {
        name: p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone()),
        path,
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        modified,
        readonly,
    })
}

/// 复制文件或递归复制目录
#[tauri::command]
pub fn copy_path(src: String, dest: String) -> Result<(), String> {
    let src_path = Path::new(&src);
    if !src_path.exists() {
        return Err(format!("Source does not exist: {}", src));
    }
    let dest_path = Path::new(&dest);
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
pub fn search_files(root: String, pattern: String, max_depth: Option<u32>) -> Result<Vec<SearchResult>, String> {
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(format!("Not a directory: {}", root));
    }
    let max_depth = max_depth.unwrap_or(20);
    let mut results = Vec::new();
    search_recursive(root_path, &pattern, 0, max_depth, &mut results)?;
    results.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir)
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
        let metadata = entry.metadata().map_err(|e| format!("Failed to get metadata: {}", e))?;

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
        .configure(
            Config::default()
                .with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| format!("Failed to configure watcher: {}", e))?;

    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {}", e))?;
    watcher
        .watch(&cwd, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to start watching: {}", e))?;

    // 存储 watcher（保持存活）
    {
        let mut w = state.watcher.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        *w = Some(watcher);
    }

    // 后台线程：接收事件 → emit 到前端
    let running_clone = running.clone();
    std::thread::spawn(move || {
        for event_res in rx {
            match event_res {
                Ok(event) => {
                    // 忽略纯 Access 事件，减少噪声
                    if matches!(
                        event.kind,
                        EventKind::Access(_)
                    ) {
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
    let mut w = state.watcher.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    *w = None; // drop watcher → stops the thread
    let mut r = state.running.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    *r = false;
    Ok(())
}
