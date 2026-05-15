use crate::services::workspace;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Git 状态条目
#[derive(Debug, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String, // "modified" | "added" | "deleted" | "untracked" | "renamed"
    pub old_path: Option<String>,
    pub staged: bool,
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

        let old_path = if status.is_index_renamed() {
            entry
                .index_to_workdir()
                .and_then(|r| r.old_file().path().map(|p| p.to_string_lossy().to_string()))
        } else {
            None
        };

        if let Some(status_str) = index_status_string(status) {
            entries.push(GitStatusEntry {
                path: path.clone(),
                status: status_str.to_string(),
                old_path: old_path.clone(),
                staged: true,
            });
        }
        if let Some(status_str) = worktree_status_string(status) {
            entries.push(GitStatusEntry {
                path,
                status: status_str.to_string(),
                old_path,
                staged: false,
            });
        }
    }

    Ok(GitStatus {
        branch,
        entries,
        ahead,
        behind,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitDiffKind {
    Worktree,
    Staged,
    All,
}

impl GitDiffKind {
    fn parse(kind: Option<String>) -> Result<Self, String> {
        match kind.as_deref().unwrap_or("all") {
            "worktree" => Ok(Self::Worktree),
            "staged" => Ok(Self::Staged),
            "all" => Ok(Self::All),
            other => Err(format!("Unsupported git diff kind: {}", other)),
        }
    }
}

/// 获取文件 diff
#[tauri::command]
pub fn git_diff(path: String, file: Option<String>, kind: Option<String>) -> Result<String, String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let diff_kind = GitDiffKind::parse(kind)?;

    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let tree = head.peel_to_tree().map_err(|e| format!("Tree: {}", e))?;

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.include_untracked(true).recurse_untracked_dirs(true);
    if let Some(ref f) = file {
        let relative = repo_relative_path(&repo, f, true)?;
        diff_opts.pathspec(relative);
    }

    let index = repo.index().map_err(|e| format!("Index: {}", e))?;
    let diff = match diff_kind {
        GitDiffKind::Worktree => repo.diff_index_to_workdir(Some(&index), Some(&mut diff_opts)),
        GitDiffKind::Staged => repo.diff_tree_to_index(Some(&tree), Some(&index), Some(&mut diff_opts)),
        GitDiffKind::All => repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut diff_opts)),
    }
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

#[tauri::command]
pub fn git_stage_files(path: String, files: Vec<String>) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;

    for file in files {
        let relative = repo_relative_path(&repo, &file, true)?;
        let full_path = repo_workdir(&repo)?.join(&relative);
        if full_path.exists() {
            index
                .add_path(&relative)
                .map_err(|e| format!("Stage {}: {}", file, e))?;
        } else {
            index
                .remove_path(&relative)
                .map_err(|e| format!("Stage deletion {}: {}", file, e))?;
        }
    }

    index.write().map_err(|e| format!("Write index: {}", e))
}

#[tauri::command]
pub fn git_unstage_files(path: String, files: Vec<String>) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let paths = repo_relative_paths(&repo, &files, true)?;
    let head = repo
        .head()
        .map_err(|e| format!("HEAD: {}", e))?
        .peel_to_commit()
        .map_err(|e| format!("Commit: {}", e))?;

    repo.reset_default(Some(head.as_object()), paths.iter())
        .map_err(|e| format!("Unstage: {}", e))
}

#[tauri::command]
pub fn git_discard_files(path: String, files: Vec<String>) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let workdir = repo_workdir(&repo)?
        .canonicalize()
        .map_err(|e| format!("Repository workdir is not accessible: {}", e))?;
    let paths = repo_relative_paths(&repo, &files, true)?;
    let statuses = repo
        .statuses(Some(git2::StatusOptions::new().include_untracked(true)))
        .map_err(|e| format!("Status: {}", e))?;

    for relative in &paths {
        let rel_str = relative.to_string_lossy().replace('\\', "/");
        let status = statuses
            .iter()
            .find(|entry| entry.path() == Some(rel_str.as_str()))
            .map(|entry| entry.status())
            .unwrap_or(git2::Status::CURRENT);
        let full_path = workdir.join(relative);
        workspace::ensure_within_workspace(&full_path)?;

        if status.is_wt_new() || status.is_index_new() {
            if full_path.is_dir() {
                std::fs::remove_dir_all(&full_path)
                    .map_err(|e| format!("Remove {}: {}", full_path.display(), e))?;
            } else if full_path.exists() {
                std::fs::remove_file(&full_path)
                    .map_err(|e| format!("Remove {}: {}", full_path.display(), e))?;
            }
            if status.is_index_new() {
                let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;
                let _ = index.remove_path(relative);
                index.write().map_err(|e| format!("Write index: {}", e))?;
            }
        }
    }

    let head = repo
        .head()
        .map_err(|e| format!("HEAD: {}", e))?
        .peel_to_commit()
        .map_err(|e| format!("Commit: {}", e))?;
    repo.reset_default(Some(head.as_object()), paths.iter())
        .map_err(|e| format!("Reset index: {}", e))?;

    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    for path in &paths {
        checkout.path(path);
    }
    repo.checkout_head(Some(&mut checkout))
        .map_err(|e| format!("Checkout: {}", e))
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

fn index_status_string(status: git2::Status) -> Option<&'static str> {
    if status.is_index_new() {
        Some("added")
    } else if status.is_index_deleted() {
        Some("deleted")
    } else if status.is_index_modified() {
        Some("modified")
    } else if status.is_index_renamed() {
        Some("renamed")
    } else {
        None
    }
}

fn worktree_status_string(status: git2::Status) -> Option<&'static str> {
    if status.is_wt_new() {
        Some("untracked")
    } else if status.is_wt_deleted() {
        Some("deleted")
    } else if status.is_wt_modified() {
        Some("modified")
    } else if status.is_wt_renamed() {
        Some("renamed")
    } else {
        None
    }
}

fn repo_workdir(repo: &git2::Repository) -> Result<&Path, String> {
    repo.workdir()
        .ok_or_else(|| "Bare repositories are not supported".to_string())
}

fn repo_relative_paths(
    repo: &git2::Repository,
    files: &[String],
    allow_missing: bool,
) -> Result<Vec<PathBuf>, String> {
    files
        .iter()
        .map(|file| repo_relative_path(repo, file, allow_missing))
        .collect()
}

fn repo_relative_path(
    repo: &git2::Repository,
    file: &str,
    allow_missing: bool,
) -> Result<PathBuf, String> {
    let workdir = repo_workdir(repo)?
        .canonicalize()
        .map_err(|e| format!("Repository workdir is not accessible: {}", e))?;
    let candidate = if Path::new(file).is_absolute() {
        PathBuf::from(file)
    } else {
        workdir.join(file)
    };
    let resolved = if candidate.exists() {
        workspace::resolve_existing(candidate.to_string_lossy().as_ref())?
    } else if allow_missing {
        workspace::resolve_for_write(candidate.to_string_lossy().as_ref())?
    } else {
        return Err(format!("File does not exist: {}", file));
    };
    resolved
        .strip_prefix(workdir)
        .map(Path::to_path_buf)
        .map_err(|_| format!("File is outside repository: {}", resolved.display()))
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
            .any(|entry| entry.path == "untracked.txt" && entry.status == "untracked" && !entry.staged));
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "staged.txt" && entry.status == "added" && entry.staged));
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

    #[test]
    fn git_stage_and_unstage_file_updates_status() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        std::fs::write(env.root.join("tracked.txt"), "changed\n").unwrap();

        git_stage_files(
            env.root.to_string_lossy().to_string(),
            vec!["tracked.txt".to_string()],
        )
        .unwrap();
        let staged_status = git_status(env.root.to_string_lossy().to_string()).unwrap();
        assert!(staged_status
            .entries
            .iter()
            .any(|entry| entry.path == "tracked.txt" && entry.staged));

        git_unstage_files(
            env.root.to_string_lossy().to_string(),
            vec!["tracked.txt".to_string()],
        )
        .unwrap();
        let unstaged_status = git_status(env.root.to_string_lossy().to_string()).unwrap();
        assert!(unstaged_status
            .entries
            .iter()
            .any(|entry| entry.path == "tracked.txt" && !entry.staged));
    }

    #[test]
    fn git_diff_can_select_staged_or_worktree_changes() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        std::fs::write(env.root.join("tracked.txt"), "staged change\n").unwrap();

        git_stage_files(
            env.root.to_string_lossy().to_string(),
            vec!["tracked.txt".to_string()],
        )
        .unwrap();
        std::fs::write(env.root.join("tracked.txt"), "worktree change\n").unwrap();

        let staged = git_diff(
            env.root.to_string_lossy().to_string(),
            Some("tracked.txt".to_string()),
            Some("staged".to_string()),
        )
        .unwrap();
        let worktree = git_diff(
            env.root.to_string_lossy().to_string(),
            Some("tracked.txt".to_string()),
            Some("worktree".to_string()),
        )
        .unwrap();
        let all = git_diff(
            env.root.to_string_lossy().to_string(),
            Some("tracked.txt".to_string()),
            Some("all".to_string()),
        )
        .unwrap();

        assert!(staged.contains("+staged change"));
        assert!(!staged.contains("+worktree change"));
        assert!(worktree.contains("-staged change"));
        assert!(worktree.contains("+worktree change"));
        assert!(all.contains("+worktree change"));
    }

    #[test]
    fn git_discard_file_restores_tracked_content() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        std::fs::write(env.root.join("tracked.txt"), "changed\n").unwrap();

        git_discard_files(
            env.root.to_string_lossy().to_string(),
            vec!["tracked.txt".to_string()],
        )
        .unwrap();

        let content = std::fs::read_to_string(env.root.join("tracked.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(content, "initial\n");
    }
}
