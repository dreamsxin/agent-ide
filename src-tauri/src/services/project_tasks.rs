use crate::services::problem_parser::{parse_terminal_problems, ProblemEntry};
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectTask {
    pub id: String,
    pub label: String,
    pub command: String,
    pub source: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunProjectTaskRequest {
    pub command: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunProjectTaskResult {
    pub command: String,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub problems: Vec<ProblemEntry>,
}

pub fn discover_project_tasks(path: Option<&str>) -> Result<Vec<ProjectTask>, String> {
    let root = task_workspace_root(path)?;
    discover_project_tasks_in_root(&root)
}

pub fn discover_project_tasks_in_root(root: &Path) -> Result<Vec<ProjectTask>, String> {
    let mut tasks = Vec::new();

    tasks.extend(discover_package_scripts(root)?);
    tasks.extend(discover_cargo_tasks(root));

    Ok(tasks)
}

pub async fn run_project_task(
    request: RunProjectTaskRequest,
) -> Result<RunProjectTaskResult, String> {
    let root = task_workspace_root(request.cwd.as_deref())?;
    run_project_command(request.command, root).await
}

/// 尽力杀掉整棵进程树。
///
/// `child.kill()` 只杀我们启的那个 shell。Windows 上 `cmd /C npm test` 的真正工作在
/// 孙子进程里（npm → node），杀掉 cmd 之后它们继续跑、继续占管道 —— 用户点了 Stop，
/// 测试还在跑。所以 Windows 上走 `taskkill /T /F`，把整棵树带走。
///
/// Unix 上 `sh -lc "cmd"` 通常会 exec 成那条命令本身，所以 `kill()` 就是杀它；真正需要
/// 进程组的情况（命令自己再 fork）留给 ROADMAP 61，那需要 `setsid`。
fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub async fn run_project_command(
    command: String,
    root: PathBuf,
) -> Result<RunProjectTaskResult, String> {
    // 用户从终端面板发起的命令没有"运行取消开关"，给一个永远为 false 的
    run_project_command_cancellable(
        command,
        root,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await
}

/// 可被取消的命令执行：取消时**杀掉子进程**，而不是等它自己跑完。
///
/// 之前这里是 `Command::output()`，它一直阻塞到进程退出。于是 Stop 之后一次
/// `npm test` 照旧跑到底：界面显示空闲，CPU 还在转，写出来的文件还在落盘。取消一个
/// 已经在跑的命令只能靠杀进程，没有别的办法。
///
/// 诚实的限制：`kill()` 杀的是我们启的那个 shell（Windows 上是 `cmd`）。它的孙子进程
/// （`npm` 拉起的 `node`）不一定跟着死 —— 要做到那一步需要 job object / 进程组，
/// 记在 ROADMAP 61 里。
pub async fn run_project_command_cancellable(
    command: String,
    root: PathBuf,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<RunProjectTaskResult, String> {
    let command = command.trim().to_string();
    if command.is_empty() {
        return Err("Task command is empty".to_string());
    }
    // 翻成这台机器的 shell 真能执行的形式。翻不了的在这里就说清为什么 —— 让 `cmd` 去报
    // 一句"找不到 'src/a"，读的人（模型或用户）要花一整轮才能想到是引号的事。
    //
    // 报出去的还是**原话**：日志和界面上显示模型/用户写的那一句，翻译只发生在执行的那一刻，
    // 否则记录里的命令和用户配置的对不上号。
    let executed = crate::services::command_text::portable_command(&command)?;

    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let start = Instant::now();
        let mut child = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", &executed])
                .current_dir(&root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        } else {
            Command::new("sh")
                .args(["-lc", &executed])
                .current_dir(&root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        }
        .map_err(|e| format!("Run task command: {}", e))?;

        // 两根管道都必须在**另外的线程**里读空：管道缓冲区满了子进程就阻塞在写上，
        // 而我们在等它退出 —— 那是互等。`output()` 帮我们做过这件事，改成 `spawn()`
        // 之后就得自己做。
        let mut out_pipe = child.stdout.take();
        let mut err_pipe = child.stderr.take();
        let out_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(pipe) = out_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buffer);
            }
            buffer
        });
        let err_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(pipe) = err_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buffer);
            }
            buffer
        });

        let mut killed = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(error) => return Err(format!("Wait for task command: {}", error)),
            }
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                kill_process_tree(&mut child);
                killed = true;
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };

        if killed {
            // 不 join 读取线程：管道还被孙子进程握着的时候 `read_to_end` 会一直阻塞，
            // 那就等于我们杀了进程还是要等它跑完 —— 这条路径上的 29 秒就是这么来的。
            // 线程会在管道关闭时自己结束；这里只欠一个缓冲区，不欠正确性。
            //
            // 报成错误而不是"退出码未知"的成功：这次检查没有结论，把它当结果会让修复
            // 循环以为检查通过了。
            return Err(format!(
                "Stopped: {:?} was killed because the run was cancelled.",
                command
            ));
        }
        // 按平台解码，不是无条件 UTF-8：中文 Windows 上 cargo/MSVC/npm 按 CP936 写字节，
        // `from_utf8_lossy` 会在模型、Problems 面板、修复提示词看到之前把它们变成 U+FFFD
        let stdout = crate::services::command_text::decode_child_output(
            &out_reader.join().unwrap_or_default(),
        );
        let stderr = crate::services::command_text::decode_child_output(
            &err_reader.join().unwrap_or_default(),
        );
        let combined = [stdout.as_str(), stderr.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let problems = parse_terminal_problems(&combined, "task");

        Ok(RunProjectTaskResult {
            command,
            exit_code: status.and_then(|status| status.code()),
            duration_ms: start.elapsed().as_millis(),
            stdout,
            stderr,
            problems,
        })
    })
    .await
    .map_err(|e| format!("Join task command: {}", e))?
}

pub fn task_workspace_root(path: Option<&str>) -> Result<PathBuf, String> {
    let root = match path {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path)
            .canonicalize()
            .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e)),
        _ => workspace::workspace_root(),
    }?;
    Ok(workspace::shell_compatible_path(root))
}

fn discover_package_scripts(root: &Path) -> Result<Vec<ProjectTask>, String> {
    let package_path = root.join("package.json");
    if !package_path.exists() {
        return Ok(Vec::new());
    }

    let content =
        std::fs::read_to_string(&package_path).map_err(|e| format!("Read package.json: {}", e))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Parse package.json: {}", e))?;

    let Some(scripts) = parsed.get("scripts").and_then(|value| value.as_object()) else {
        return Ok(Vec::new());
    };

    let mut tasks: Vec<ProjectTask> = scripts
        .iter()
        .filter_map(|(name, command)| {
            let command = command.as_str()?;
            Some(ProjectTask {
                id: format!("npm:{}", name),
                label: name.to_string(),
                command: format!("npm run {}", name),
                source: "package.json".to_string(),
                description: command.to_string(),
            })
        })
        .collect();

    tasks.sort_by_key(|task| score_package_script(&task.label));
    Ok(tasks)
}

fn discover_cargo_tasks(root: &Path) -> Vec<ProjectTask> {
    let mut tasks = Vec::new();

    if root.join("Cargo.toml").exists() {
        tasks.extend(cargo_task_set(""));
    }

    if root.join("src-tauri").join("Cargo.toml").exists() {
        tasks.extend(cargo_task_set("src-tauri"));
    }

    tasks
}

fn cargo_task_set(directory: &str) -> Vec<ProjectTask> {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("cd {}; ", directory)
    };
    let suffix = if directory.is_empty() { "" } else { "; cd .." };
    let source = if directory.is_empty() {
        "Cargo.toml".to_string()
    } else {
        format!("{}/Cargo.toml", directory)
    };

    [
        (
            "check",
            "Cargo Check",
            "cargo check",
            "Check Rust project without building artifacts.",
        ),
        ("test", "Cargo Test", "cargo test", "Run Rust tests."),
        ("run", "Cargo Run", "cargo run", "Run the Rust binary."),
    ]
    .into_iter()
    .map(|(id, label, command, description)| ProjectTask {
        id: format!("cargo:{}:{}", directory, id),
        label: label.to_string(),
        command: format!("{}{}{}", prefix, command, suffix),
        source: source.clone(),
        description: description.to_string(),
    })
    .collect()
}

fn score_package_script(name: &str) -> usize {
    match name {
        "workflow" => 0,
        "dev" => 1,
        "build" => 2,
        "test" => 3,
        "lint" => 4,
        "typecheck" | "check" => 5,
        "preview" => 6,
        _ => 10,
    }
}

#[cfg(test)]
// 同上：环境变量测试守卫需要跨 await 持有同步锁。
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// 取消要杀掉子进程，而不是等它跑完。
    ///
    /// 断言落在"没有等它自己结束"这个性质上，而不是具体耗时：命令用的是一个会跑
    /// 半分钟的命令，如果实现退回成 `output()`，这条测试会因为超时而暴露出来。
    #[test]
    fn a_cancelled_command_is_killed_instead_of_waited_out() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        // 两个平台上都跑约 30 秒
        let command = if cfg!(windows) {
            "ping -n 30 127.0.0.1"
        } else {
            "sleep 30"
        };
        let started = Instant::now();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_project_command_cancellable(
                command.to_string(),
                std::env::temp_dir(),
                cancel,
            ))
            .unwrap_err();

        assert!(error.contains("Stopped"), "{}", error);
        // 被杀掉的命令没有结论，所以是错误而不是"退出码未知"的成功结果
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
    }

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("agent-ide-tasks-test-{}", Uuid::new_v4()));
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
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(
                self.root
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| self.root.clone()),
            );
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    #[test]
    fn discover_project_tasks_reads_package_scripts() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write(
            "package.json",
            r#"{"scripts":{"test":"vitest","build":"vite build","dev":"vite","workflow":"node workflow-smoke.js"}}"#,
        );

        let tasks = discover_project_tasks(None).unwrap();

        assert_eq!(tasks[0].label, "workflow");
        assert!(tasks
            .iter()
            .any(|task| task.id == "npm:dev" && task.command == "npm run dev"));
        assert!(tasks.iter().any(|task| task.id == "npm:build"));
        assert!(tasks.iter().any(|task| task.id == "npm:test"));
    }

    #[test]
    fn discover_project_tasks_uses_explicit_workspace_path() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        let explicit = env.root.join("nested-app");
        std::fs::create_dir_all(&explicit).unwrap();
        std::fs::write(
            explicit.join("package.json"),
            r#"{"scripts":{"test":"vitest","dev":"vite --host"}}"#,
        )
        .unwrap();

        let tasks = discover_project_tasks(Some(explicit.to_string_lossy().as_ref())).unwrap();

        assert!(tasks.iter().any(|task| task.id == "npm:dev"));
        assert!(tasks.iter().any(|task| task.command == "npm run test"));
    }

    #[test]
    fn discover_project_tasks_detects_src_tauri_cargo() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write(
            "src-tauri/Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );

        let tasks = discover_project_tasks(None).unwrap();

        assert!(tasks
            .iter()
            .any(|task| task.command == "cd src-tauri; cargo test; cd .."));
    }

    #[tokio::test]
    async fn run_project_command_returns_exit_code_and_output() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        let command = if cfg!(windows) {
            "echo hello-task".to_string()
        } else {
            "printf hello-task".to_string()
        };

        let result = run_project_command(command, env.root.clone())
            .await
            .unwrap();

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello-task"));
    }

    #[tokio::test]
    async fn run_project_command_parses_problem_output() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        let command = if cfg!(windows) {
            "echo src\\main.rs:12:4: error: expected expression".to_string()
        } else {
            "printf 'src/main.rs:12:4: error: expected expression'".to_string()
        };

        let result = run_project_command(command, env.root.clone())
            .await
            .unwrap();

        assert_eq!(result.problems.len(), 1);
        assert_eq!(result.problems[0].file, "src/main.rs");
    }
}
