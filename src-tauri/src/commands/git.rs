use crate::services::workspace;
use serde::Serialize;

/// Git 状态条目
#[derive(Debug, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String, // "modified" | "added" | "deleted" | "untracked" | "renamed"
    pub old_path: Option<String>,
}

/// Git 状态汇总
#[derive(Debug, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub entries: Vec<GitStatusEntry>,
    pub ahead: usize,
    pub behind: usize,
}

/// 获取 Git 仓库状态
#[tauri::command]
pub fn git_status(path: String) -> Result<GitStatus, String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;

    // 获取当前分支名
    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let branch = head.shorthand().unwrap_or("HEAD").to_string();

    // 获取 ahead/behind
    let mut ahead = 0;
    let mut behind = 0;
    if let Ok(upstream) = repo.revparse_single("@{upstream}") {
        let local = head.peel_to_commit().map_err(|e| e.to_string())?;
        let upstream_commit = upstream.peel_to_commit().map_err(|e| e.to_string())?;
        let (a, b) = repo
            .graph_ahead_behind(local.id(), upstream_commit.id())
            .map_err(|e| e.to_string())?;
        ahead = a as usize;
        behind = b as usize;
    }

    // 获取状态
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_untracked(true)
        .renames_head_to_index(true);

    let statuses = repo
        .statuses(Some(&mut status_opts))
        .map_err(|e| format!("Status: {}", e))?;

    let mut entries = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry.path().unwrap_or("").to_string();

        let status_str = if status.is_wt_new() {
            "untracked"
        } else if status.is_index_new() {
            "added"
        } else if status.is_index_deleted() || status.is_wt_deleted() {
            "deleted"
        } else if status.is_index_modified() || status.is_wt_modified() {
            "modified"
        } else if status.is_index_renamed() || status.is_wt_renamed() {
            "renamed"
        } else {
            continue; // skip clean files
        };

        let old_path = if status.is_index_renamed() {
            entry
                .index_to_workdir()
                .and_then(|r| r.old_file().path().map(|p| p.to_string_lossy().to_string()))
        } else {
            None
        };

        entries.push(GitStatusEntry {
            path,
            status: status_str.to_string(),
            old_path,
        });
    }

    Ok(GitStatus {
        branch,
        entries,
        ahead,
        behind,
    })
}

/// 获取文件 diff
#[tauri::command]
pub fn git_diff(path: String, file: Option<String>) -> Result<String, String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;

    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let tree = head.peel_to_tree().map_err(|e| format!("Tree: {}", e))?;

    let mut diff_opts = git2::DiffOptions::new();
    if let Some(ref f) = file {
        let file_path = workspace::resolve_existing(f)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| "Bare repositories are not supported".to_string())?;
        let relative = file_path
            .strip_prefix(workdir)
            .map_err(|_| format!("File is outside repository: {}", file_path.display()))?;
        diff_opts.pathspec(relative);
    }

    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut diff_opts))
        .map_err(|e| format!("Diff: {}", e))?;

    let mut patch_buf = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_, _, line| {
        let origin = match line.origin() {
            '+' => '+',
            '-' => '-',
            ' ' => ' ',
            _ => ' ',
        };
        let content = String::from_utf8_lossy(line.content());
        patch_buf.push((origin, content.to_string()));
        true
    })
    .map_err(|e| format!("Diff print: {}", e))?;

    let output: String = patch_buf
        .iter()
        .map(|(o, c)| format!("{}{}", o, c))
        .collect();

    Ok(output)
}

/// 提交变更
#[tauri::command]
pub fn git_commit(
    path: String,
    message: String,
    files: Option<Vec<String>>,
) -> Result<String, String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;

    let signature = repo
        .signature()
        .map_err(|e| format!("No git user configured: {}", e))?;

    let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;

    // Stage files
    if let Some(file_list) = files {
        for file in &file_list {
            let file_path = workspace::resolve_existing(file)?;
            let workdir = repo
                .workdir()
                .ok_or_else(|| "Bare repositories are not supported".to_string())?;
            let relative = file_path
                .strip_prefix(workdir)
                .map_err(|_| format!("File is outside repository: {}", file_path.display()))?;
            index
                .add_path(relative)
                .map_err(|e| format!("Add {}: {}", file, e))?;
        }
    } else {
        // Stage all
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| format!("Add all: {}", e))?;
    }

    index.write().map_err(|e| format!("Write index: {}", e))?;

    let oid = index
        .write_tree()
        .map_err(|e| format!("Write tree: {}", e))?;
    let tree = repo
        .find_tree(oid)
        .map_err(|e| format!("Find tree: {}", e))?;

    let parent_commits = if let Ok(ref head_ref) = repo.head() {
        vec![head_ref
            .peel_to_commit()
            .map_err(|e| format!("Parent commit: {}", e))?]
    } else {
        Vec::new()
    };

    let parents: Vec<&git2::Commit> = parent_commits.iter().collect();

    let commit_oid = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            &parents,
        )
        .map_err(|e| format!("Commit: {}", e))?;

    Ok(commit_oid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestRepo {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let base = std::env::current_dir()
                .unwrap()
                .join("target")
                .join("agent-ide-git-tests")
                .join(Uuid::new_v4().to_string());
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();

            let repo = git2::Repository::init(&root).unwrap();
            let tracked = root.join("tracked.txt");
            std::fs::write(&tracked, "initial\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("tracked.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("Agent IDE Test", "agent@example.com").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();

            Self { root, config_dir }
        }

        fn outside_path(&self, relative: &str) -> PathBuf {
            let path = self
                .root
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("outside")
                .join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            path
        }
    }

    impl Drop for TestRepo {
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
    fn git_status_distinguishes_added_and_untracked() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        std::fs::write(env.root.join("untracked.txt"), "new\n").unwrap();
        std::fs::write(env.root.join("staged.txt"), "staged\n").unwrap();

        let repo = git2::Repository::open(&env.root).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();

        let status = git_status(env.root.to_string_lossy().to_string()).unwrap();

        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "untracked.txt" && entry.status == "untracked"));
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "staged.txt" && entry.status == "added"));
    }

    #[test]
    fn git_status_rejects_paths_outside_workspace() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        let outside = env.outside_path("repo");
        std::fs::create_dir_all(&outside).unwrap();
        git2::Repository::init(&outside).unwrap();

        let err = git_status(outside.to_string_lossy().to_string()).unwrap_err();

        assert!(err.contains("outside workspace"));
    }
}
