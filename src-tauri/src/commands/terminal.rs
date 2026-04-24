use portable_pty::{native_pty_system, PtySize, CommandBuilder};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

/// 终端实例
struct TerminalInstance {
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    cancel_tx: mpsc::Sender<()>,
}

/// 全局终端管理器
pub struct TerminalManager {
    terminals: Mutex<HashMap<String, TerminalInstance>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
        }
    }
}

/// 生成 PTY 终端
#[tauri::command]
pub async fn spawn_terminal(
    app: AppHandle,
    terminal_manager: tauri::State<'_, TerminalManager>,
    id: String,
) -> Result<(), String> {
    // 检查是否已存在
    {
        let terminals = terminal_manager
            .terminals
            .lock()
            .map_err(|e| e.to_string())?;
        if terminals.contains_key(&id) {
            return Err(format!("Terminal {} already exists", id));
        }
    }

    let pty_system = native_pty_system();

    // 创建 PTY
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    // 构建 shell 命令
    #[cfg(target_os = "windows")]
    let cmd = CommandBuilder::new("cmd.exe");

    #[cfg(not(target_os = "windows"))]
    let cmd = CommandBuilder::new(
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
    );

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    // 丢弃 slave，master 端持有
    drop(pair.slave);

    let master = pair.master;
    let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

    // 获取 reader（克隆引用，不消耗 master）
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone reader: {}", e))?;

    let app_clone = app.clone();
    let id_clone = id.clone();

    // 后台线程读取 PTY 输出 → 推送事件到前端
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            // 检查是否取消
            if cancel_rx.try_recv().is_ok() {
                break;
            }

            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    let payload = serde_json::json!({ "id": &id_clone, "data": data });
                    let _ = app_clone.emit("terminal-output", payload);
                }
                Err(_) => {
                    // 短暂等待后重试
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    });

    // 存储终端实例
    {
        let mut terminals = terminal_manager
            .terminals
            .lock()
            .map_err(|e| e.to_string())?;
        terminals.insert(
            id,
            TerminalInstance {
                pty_master: master,
                _child: child,
                cancel_tx,
            },
        );
    }

    Ok(())
}

/// 写入输入到终端
#[tauri::command]
pub async fn write_to_terminal(
    terminal_manager: tauri::State<'_, TerminalManager>,
    id: String,
    data: String,
) -> Result<(), String> {
    let mut terminals = terminal_manager
        .terminals
        .lock()
        .map_err(|e| e.to_string())?;
    let instance = terminals
        .get_mut(&id)
        .ok_or_else(|| format!("Terminal {} not found", id))?;

    let mut writer = instance
        .pty_master
        .take_writer()
        .map_err(|e| format!("Failed to get writer: {}", e))?;
    writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("Failed to write to PTY: {}", e))?;

    Ok(())
}

/// 调整终端尺寸
#[tauri::command]
pub async fn resize_terminal(
    terminal_manager: tauri::State<'_, TerminalManager>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut terminals = terminal_manager
        .terminals
        .lock()
        .map_err(|e| e.to_string())?;
    let instance = terminals
        .get_mut(&id)
        .ok_or_else(|| format!("Terminal {} not found", id))?;

    instance
        .pty_master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to resize PTY: {}", e))?;

    Ok(())
}

/// 关闭终端
#[tauri::command]
pub async fn kill_terminal(
    terminal_manager: tauri::State<'_, TerminalManager>,
    id: String,
) -> Result<(), String> {
    let mut terminals = terminal_manager
        .terminals
        .lock()
        .map_err(|e| e.to_string())?;
    if terminals.remove(&id).is_some() {
        // 实例被移除时 pty_master 会被 drop，关闭 PTY 连接
    }
    Ok(())
}
