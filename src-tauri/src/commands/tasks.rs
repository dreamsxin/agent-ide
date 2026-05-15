use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::path::Path;
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
}

#[tauri::command]
pub fn discover_project_tasks(path: Option<String>) -> Result<Vec<ProjectTask>, String> {
    let root = task_workspace_root(path.as_deref())?;
    let mut tasks = Vec::new();

    tasks.extend(discover_package_scripts(&root)?);
    tasks.extend(discover_cargo_tasks(&root));

    Ok(tasks)
}

#[tauri::command]
pub async fn run_project_task(request: RunProjectTaskRequest) -> Result<RunProjectTaskResult, String> {
    let root = task_workspace_root(request.cwd.as_deref())?;
    let command = request.command.trim().to_string();
    if command.is_empty() {
        return Err("Task command is empty".to_string());
    }

    tokio::task::spawn_blocking(move || {
        let start = Instant::now();
        let output = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&root)
                .output()
        } else {
            Command::new("sh")
                .args(["-lc", &command])
                .current_dir(&root)
                .output()
        }
        .map_err(|e| format!("Run task command: {}", e))?;

        Ok(RunProjectTaskResult {
            command,
            exit_code: output.status.code(),
            duration_ms: start.elapsed().as_millis(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    })
    .await
    .map_err(|e| format!("Join task command: {}", e))?
}

fn task_workspace_root(path: Option<&str>) -> Result<std::path::PathBuf, String> {
    let root = match path {
        Some(path) if !path.trim().is_empty() => std::path::PathBuf::from(path)
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

    let content = std::fs::read_to_string(&package_path)
        .map_err(|e| format!("Read package.json: {}", e))?;
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

    tasks.sort_by(|a, b| score_package_script(&a.label).cmp(&score_package_script(&b.label)));
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
        ("check", "Cargo Check", "cargo check", "Check Rust project without building artifacts."),
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
        "dev" => 0,
        "build" => 1,
        "test" => 2,
        "lint" => 3,
        "typecheck" | "check" => 4,
        "preview" => 5,
        _ => 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestEnv {
        root: std::path::PathBuf,
        config_dir: std::path::PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!("agent-ide-tasks-test-{}", Uuid::new_v4()));
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
            r#"{"scripts":{"test":"vitest","build":"vite build","dev":"vite"}}"#,
        );

        let tasks = discover_project_tasks(None).unwrap();

        assert!(tasks.iter().any(|task| task.id == "npm:dev" && task.command == "npm run dev"));
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

        let tasks = discover_project_tasks(Some(explicit.to_string_lossy().to_string())).unwrap();

        assert!(tasks.iter().any(|task| task.id == "npm:dev"));
        assert!(tasks.iter().any(|task| task.command == "npm run test"));
    }

    #[test]
    fn discover_project_tasks_detects_src_tauri_cargo() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src-tauri/Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");

        let tasks = discover_project_tasks(None).unwrap();

        assert!(tasks
            .iter()
            .any(|task| task.command == "cd src-tauri; cargo test; cd .."));
    }
}
