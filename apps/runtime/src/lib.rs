use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapability {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBootstrap {
    pub app_name: &'static str,
    pub runtime: &'static str,
    pub capabilities: Vec<RuntimeCapability>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceState {
    pub root: String,
    pub entries: Vec<WorkspaceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDocument {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFileRequest {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitState {
    pub branch: String,
    pub dirty: bool,
    pub summary: String,
    pub changes: Vec<GitChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitChange {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub fn bootstrap() -> RuntimeBootstrap {
    RuntimeBootstrap {
        app_name: "Agent IDE",
        runtime: "rust",
        capabilities: vec![
            RuntimeCapability {
                id: "workspace.open",
                label: "Open workspace",
            },
            RuntimeCapability {
                id: "workspace.read",
                label: "Read files",
            },
            RuntimeCapability {
                id: "workspace.write",
                label: "Write files",
            },
            RuntimeCapability {
                id: "git.status",
                label: "Inspect Git status",
            },
            RuntimeCapability {
                id: "command.run",
                label: "Run workspace command",
            },
            RuntimeCapability {
                id: "agent.run",
                label: "Run agent task",
            },
        ],
    }
}

pub fn open_workspace(root: &str) -> Result<WorkspaceState, String> {
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return Err("selected path is not a directory".into());
    }

    let mut entries = Vec::new();
    for entry in WalkDir::new(&root_path)
        .min_depth(1)
        .max_depth(3)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if should_skip(path) {
            continue;
        }

        let relative = match path.strip_prefix(&root_path) {
            Ok(relative) => relative,
            Err(_) => continue,
        };

        entries.push(WorkspaceEntry {
            path: normalize_separators(relative),
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: if path.is_dir() { "directory" } else { "file" },
        });
    }

    Ok(WorkspaceState {
        root: root_path.to_string_lossy().into_owned(),
        entries,
    })
}

pub fn read_file(root: &str, relative_path: &str) -> Result<FileDocument, String> {
    let full_path = resolve_workspace_path(root, relative_path)?;
    let contents = fs::read_to_string(&full_path).map_err(|err| err.to_string())?;

    Ok(FileDocument {
        path: normalize_separators(Path::new(relative_path)),
        contents,
    })
}

pub fn save_file(root: &str, request: SaveFileRequest) -> Result<(), String> {
    let full_path = resolve_workspace_path(root, &request.path)?;
    fs::write(full_path, request.contents).map_err(|err| err.to_string())
}

pub fn git_state(root: &str) -> GitState {
    let branch = git_output(root, ["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "no-git".into());
    let status = git_output(root, ["status", "--short"]).unwrap_or_default();
    let changes = parse_git_changes(&status);
    let dirty = !status.trim().is_empty();
    let summary = if branch == "no-git" {
        "Git repository not detected".into()
    } else if dirty {
        format!("{} changed item(s)", changes.len())
    } else {
        "Working tree clean".into()
    };

    GitState {
        branch,
        dirty,
        summary,
        changes,
    }
}

pub fn run_command(root: &str, request: CommandRequest) -> Result<CommandResult, String> {
    let workspace_root = PathBuf::from(root);
    if !workspace_root.is_dir() {
        return Err("workspace root is not a directory".into());
    }

    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", request.command.as_str()])
            .current_dir(&workspace_root)
            .output()
    } else {
        Command::new("sh")
            .args(["-lc", request.command.as_str()])
            .current_dir(&workspace_root)
            .output()
    }
    .map_err(|err| err.to_string())?;

    Ok(CommandResult {
        command: request.command,
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn parse_git_changes(status: &str) -> Vec<GitChange> {
    status
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }

            let status_code = line[..2].trim().to_string();
            let raw_path = line[3..].trim();
            let path = raw_path
                .split(" -> ")
                .last()
                .unwrap_or(raw_path)
                .replace('\\', "/");

            Some(GitChange {
                path,
                status: if status_code.is_empty() {
                    "modified".into()
                } else {
                    status_code
                },
            })
        })
        .collect()
}

fn git_output<const N: usize>(root: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn resolve_workspace_path(root: &str, relative_path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(root);
    let candidate = root.join(relative_path);
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    let canonical_candidate = candidate.canonicalize().map_err(|err| err.to_string())?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err("path escapes workspace root".into());
    }

    Ok(canonical_candidate)
}

fn normalize_separators(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn should_skip(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | "node_modules" | "target" | "dist")
    })
}
