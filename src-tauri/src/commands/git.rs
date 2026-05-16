use crate::services::{credentials, workspace};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Git 状态条目
#[derive(Debug, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String, // "modified" | "added" | "deleted" | "untracked" | "renamed" | "conflicted"
    pub old_path: Option<String>,
    pub staged: bool,
}

#[derive(Debug, Serialize)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitCredentials {
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub save: bool,
}

/// Git 状态汇总
#[derive(Debug, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub entries: Vec<GitStatusEntry>,
    pub ahead: usize,
    pub behind: usize,
    pub upstream: Option<String>,
    pub branches: Vec<GitBranch>,
    pub conflicts: Vec<String>,
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
    let upstream_name = head
        .shorthand()
        .and_then(|_| current_upstream_name(&repo).ok())
        .flatten();
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
    let mut conflicts = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry.path().unwrap_or("").to_string();
        if status.is_conflicted() {
            conflicts.push(path.clone());
            entries.push(GitStatusEntry {
                path,
                status: "conflicted".to_string(),
                old_path: None,
                staged: false,
            });
            continue;
        }

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

    let branches = list_branches(&repo, Some(&branch))?;

    Ok(GitStatus {
        branch,
        entries,
        ahead,
        behind,
        upstream: upstream_name,
        branches,
        conflicts,
    })
}

#[tauri::command]
pub fn git_checkout_branch(path: String, branch: String, create: bool) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let branch = validate_branch_name(&branch)?;

    if create {
        let head_commit = repo
            .head()
            .map_err(|e| format!("HEAD: {}", e))?
            .peel_to_commit()
            .map_err(|e| format!("Commit: {}", e))?;
        repo.branch(&branch, &head_commit, false)
            .map_err(|e| format!("Create branch {}: {}", branch, e))?;
    } else {
        repo.find_branch(&branch, git2::BranchType::Local)
            .map_err(|e| format!("Local branch {} not found: {}", branch, e))?;
    }

    repo.set_head(&format!("refs/heads/{}", branch))
        .map_err(|e| format!("Set HEAD: {}", e))?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_head(Some(&mut checkout))
        .map_err(|e| format!("Checkout {}: {}", branch, e))
}

#[tauri::command]
pub fn git_checkout_remote_branch(
    path: String,
    remote_branch: String,
    local_branch: Option<String>,
) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let remote_branch = remote_branch.trim();
    let reference_name = format!("refs/remotes/{}", remote_branch);
    let reference = repo
        .find_reference(&reference_name)
        .map_err(|e| format!("Remote branch {} not found: {}", remote_branch, e))?;
    let commit = reference
        .peel_to_commit()
        .map_err(|e| format!("Remote branch commit: {}", e))?;
    let local_name = local_branch
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            remote_branch
                .rsplit('/')
                .next()
                .unwrap_or(remote_branch)
                .to_string()
        });
    let local_name = validate_branch_name(&local_name)?;

    repo.branch(&local_name, &commit, false)
        .map_err(|e| format!("Create tracking branch {}: {}", local_name, e))?;
    let remote_name = remote_branch.split('/').next().unwrap_or("origin");
    let remote_short = remote_branch
        .strip_prefix(&format!("{}/", remote_name))
        .unwrap_or(remote_branch);
    let mut config = repo.config().map_err(|e| format!("Git config: {}", e))?;
    config
        .set_str(&format!("branch.{}.remote", local_name), remote_name)
        .map_err(|e| format!("Set branch remote: {}", e))?;
    config
        .set_str(
            &format!("branch.{}.merge", local_name),
            &format!("refs/heads/{}", remote_short),
        )
        .map_err(|e| format!("Set branch merge: {}", e))?;

    repo.set_head(&format!("refs/heads/{}", local_name))
        .map_err(|e| format!("Set HEAD: {}", e))?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_head(Some(&mut checkout))
        .map_err(|e| format!("Checkout {}: {}", local_name, e))
}

#[tauri::command]
pub fn git_fetch(
    path: String,
    remote: Option<String>,
    credentials: Option<GitCredentials>,
) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let remote_name = remote.unwrap_or_else(|| "origin".to_string());
    let mut remote = repo
        .find_remote(&remote_name)
        .map_err(|e| format!("Remote {}: {}", remote_name, e))?;
    let mut options = remote_callbacks(
        &repo,
        credentials,
        Some(remote.url().unwrap_or_default().to_string()),
    )?;
    remote
        .fetch(&[] as &[&str], Some(&mut options), None)
        .map_err(|e| format!("Fetch {}: {}", remote_name, e))
}

#[tauri::command]
pub fn git_pull(
    path: String,
    remote: Option<String>,
    credentials: Option<GitCredentials>,
) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let current_branch = current_branch_name(&repo)?;
    if has_uncommitted_changes(&repo)? {
        return Err("Pull requires a clean working tree in this first version.".to_string());
    }

    git_fetch(path.to_string_lossy().to_string(), remote, credentials)?;
    let upstream = current_upstream_name(&repo)?
        .ok_or_else(|| format!("Branch {} has no upstream", current_branch))?;
    let upstream_ref = repo
        .find_reference(&upstream)
        .map_err(|e| format!("Find upstream {}: {}", upstream, e))?;
    let upstream_annotated = repo
        .reference_to_annotated_commit(&upstream_ref)
        .map_err(|e| format!("Upstream commit: {}", e))?;
    let (analysis, _) = repo
        .merge_analysis(&[&upstream_annotated])
        .map_err(|e| format!("Merge analysis: {}", e))?;
    if analysis.is_up_to_date() {
        return Ok(());
    }
    if !analysis.is_fast_forward() {
        return Err("Pull requires a fast-forward merge in this first version.".to_string());
    }
    let upstream_commit = upstream_ref
        .peel_to_commit()
        .map_err(|e| format!("Upstream commit: {}", e))?;
    let mut reference = repo
        .find_reference(&format!("refs/heads/{}", current_branch))
        .map_err(|e| format!("Find branch {}: {}", current_branch, e))?;
    reference
        .set_target(upstream_commit.id(), "fast-forward pull")
        .map_err(|e| format!("Fast-forward pull: {}", e))?;
    repo.set_head(&format!("refs/heads/{}", current_branch))
        .map_err(|e| format!("Set HEAD: {}", e))?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_head(Some(&mut checkout))
        .map_err(|e| format!("Checkout pulled HEAD: {}", e))
}

#[tauri::command]
pub fn git_push(
    path: String,
    remote: Option<String>,
    credentials: Option<GitCredentials>,
) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let branch = current_branch_name(&repo)?;
    let remote_name = remote.unwrap_or_else(|| "origin".to_string());
    let mut remote = repo
        .find_remote(&remote_name)
        .map_err(|e| format!("Remote {}: {}", remote_name, e))?;
    let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);
    let mut options = git2::PushOptions::new();
    let callbacks = git_remote_callbacks(
        &repo,
        credentials,
        Some(remote.url().unwrap_or_default().to_string()),
    )?;
    options.remote_callbacks(callbacks);
    remote
        .push(&[refspec.as_str()], Some(&mut options))
        .map_err(|e| format!("Push {}: {}", remote_name, e))
}

#[tauri::command]
pub fn git_resolve_conflict(path: String, file: String, resolution: String) -> Result<(), String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let relative = repo_relative_path(&repo, &file, true)?;
    let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;
    let conflict = find_index_conflict(&index, &relative)?
        .ok_or_else(|| format!("No conflict found for {}", file))?;
    let ours = conflict
        .our
        .as_ref()
        .ok_or_else(|| format!("Conflict has no current side: {}", file))?;
    let theirs = conflict
        .their
        .as_ref()
        .ok_or_else(|| format!("Conflict has no incoming side: {}", file))?;
    let ours_content = blob_text(&repo, ours.id)?;
    let theirs_content = blob_text(&repo, theirs.id)?;
    let content = match resolution.as_str() {
        "current" => ours_content,
        "incoming" => theirs_content,
        "both" => format!(
            "{}{}{}",
            ours_content,
            if ours_content.ends_with('\n') {
                ""
            } else {
                "\n"
            },
            theirs_content
        ),
        other => return Err(format!("Unsupported conflict resolution: {}", other)),
    };
    let target = repo_workdir(&repo)?.join(&relative);
    workspace::ensure_within_workspace(&target)?;
    std::fs::write(&target, content).map_err(|e| format!("Write resolved file: {}", e))?;
    index
        .add_path(&relative)
        .map_err(|e| format!("Stage resolved file: {}", e))?;
    index.write().map_err(|e| format!("Write index: {}", e))
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
pub fn git_diff(
    path: String,
    file: Option<String>,
    kind: Option<String>,
) -> Result<String, String> {
    let path = workspace::resolve_existing(&path)?;
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;
    let diff_kind = GitDiffKind::parse(kind)?;

    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let tree = head.peel_to_tree().map_err(|e| format!("Tree: {}", e))?;

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts
        .include_untracked(true)
        .recurse_untracked_dirs(true);
    if let Some(ref f) = file {
        let relative = repo_relative_path(&repo, f, true)?;
        diff_opts.pathspec(relative);
    }

    let index = repo.index().map_err(|e| format!("Index: {}", e))?;
    let diff = match diff_kind {
        GitDiffKind::Worktree => repo.diff_index_to_workdir(Some(&index), Some(&mut diff_opts)),
        GitDiffKind::Staged => {
            repo.diff_tree_to_index(Some(&tree), Some(&index), Some(&mut diff_opts))
        }
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
        .map(workspace::shell_compatible_path)
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

fn current_branch_name(repo: &git2::Repository) -> Result<String, String> {
    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    head.shorthand()
        .map(str::to_string)
        .ok_or_else(|| "Detached HEAD is not supported for this operation".to_string())
}

fn current_upstream_name(repo: &git2::Repository) -> Result<Option<String>, String> {
    let branch_name = current_branch_name(repo)?;
    let branch = repo
        .find_branch(&branch_name, git2::BranchType::Local)
        .map_err(|e| format!("Branch {}: {}", branch_name, e))?;
    match branch.upstream() {
        Ok(upstream) => Ok(upstream.get().name().map(|name| name.to_string())),
        Err(_) => {
            let config = repo.config().map_err(|e| format!("Git config: {}", e))?;
            let remote = config
                .get_string(&format!("branch.{}.remote", branch_name))
                .ok();
            let merge = config
                .get_string(&format!("branch.{}.merge", branch_name))
                .ok();
            Ok(match (remote, merge) {
                (Some(remote), Some(merge)) => merge
                    .strip_prefix("refs/heads/")
                    .map(|name| format!("refs/remotes/{}/{}", remote, name)),
                _ => None,
            })
        }
    }
}

fn list_branches(repo: &git2::Repository, current: Option<&str>) -> Result<Vec<GitBranch>, String> {
    let mut branches = Vec::new();
    let iter = repo
        .branches(None)
        .map_err(|e| format!("List branches: {}", e))?;
    for branch_result in iter {
        let (branch, branch_type) = branch_result.map_err(|e| format!("Branch: {}", e))?;
        let Some(name) = branch.name().map_err(|e| format!("Branch name: {}", e))? else {
            continue;
        };
        branches.push(GitBranch {
            name: name.to_string(),
            current: current == Some(name) && branch_type == git2::BranchType::Local,
            remote: branch_type == git2::BranchType::Remote,
        });
    }
    branches.sort_by(|a, b| a.remote.cmp(&b.remote).then_with(|| a.name.cmp(&b.name)));
    Ok(branches)
}

fn validate_branch_name(branch: &str) -> Result<String, String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return Err("Branch name is empty".to_string());
    }
    if trimmed.contains("..")
        || trimmed.starts_with('-')
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_whitespace)
    {
        return Err(format!("Invalid branch name: {}", branch));
    }
    Ok(trimmed.to_string())
}

fn has_uncommitted_changes(repo: &git2::Repository) -> Result<bool, String> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| format!("Status: {}", e))?;
    Ok(!statuses.is_empty())
}

fn remote_callbacks(
    repo: &git2::Repository,
    credentials: Option<GitCredentials>,
    remote_url: Option<String>,
) -> Result<git2::FetchOptions<'static>, String> {
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(git_remote_callbacks(repo, credentials, remote_url)?);
    Ok(options)
}

fn git_remote_callbacks(
    repo: &git2::Repository,
    credentials: Option<GitCredentials>,
    remote_url: Option<String>,
) -> Result<git2::RemoteCallbacks<'static>, String> {
    let mut callbacks = git2::RemoteCallbacks::new();
    let config = repo.config().map_err(|e| format!("Git config: {}", e))?;
    callbacks.credentials(move |url, username_from_url, allowed| {
        if allowed.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                return git2::Cred::ssh_key_from_agent(username);
            }
        }
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            let remote_url = remote_url
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(url);
            if let Some(git_credentials) = credentials.as_ref() {
                if let Some((username, password)) = username_password_from_request(git_credentials)
                {
                    if git_credentials.save {
                        let _ = store_git_credentials(remote_url, username, password);
                    }
                    return git2::Cred::userpass_plaintext(username, password);
                }
            }
            if let Some((username, password)) =
                read_stored_git_credentials(remote_url, username_from_url)
            {
                return git2::Cred::userpass_plaintext(&username, &password);
            }
            if let (Ok(username), Ok(password)) =
                (std::env::var("GIT_USERNAME"), std::env::var("GIT_PASSWORD"))
            {
                return git2::Cred::userpass_plaintext(&username, &password);
            }
        }
        git2::Cred::credential_helper(&config, url, username_from_url)
    });
    Ok(callbacks)
}

fn username_password_from_request(credentials: &GitCredentials) -> Option<(&str, &str)> {
    let username = credentials
        .username
        .as_deref()
        .filter(|value| !value.trim().is_empty())?;
    let password = credentials
        .password
        .as_deref()
        .filter(|value| !value.trim().is_empty())?;
    Some((username, password))
}

fn store_git_credentials(remote_url: &str, username: &str, password: &str) -> Result<(), String> {
    credentials::store_secret(
        &credentials::git_credential_ref(remote_url),
        &serialize_git_credentials(username, password),
    )
}

fn read_stored_git_credentials(
    remote_url: &str,
    username_from_url: Option<&str>,
) -> Option<(String, String)> {
    let stored = credentials::read_secret(&credentials::git_credential_ref(remote_url)).ok()?;
    parse_git_credentials_secret(&stored, username_from_url)
}

fn serialize_git_credentials(username: &str, password: &str) -> String {
    format!("{}\n{}", username, password)
}

fn parse_git_credentials_secret(
    stored: &str,
    username_from_url: Option<&str>,
) -> Option<(String, String)> {
    let (username, password) = stored.split_once('\n')?;
    let username = if username.trim().is_empty() {
        username_from_url.unwrap_or("").to_string()
    } else {
        username.to_string()
    };
    if username.trim().is_empty() || password.trim().is_empty() {
        return None;
    }
    Some((username, password.to_string()))
}

fn find_index_conflict(
    index: &git2::Index,
    relative: &Path,
) -> Result<Option<git2::IndexConflict>, String> {
    let target = relative.to_string_lossy().replace('\\', "/");
    let conflicts = index.conflicts().map_err(|e| format!("Conflicts: {}", e))?;
    for conflict in conflicts {
        let conflict = conflict.map_err(|e| format!("Conflict entry: {}", e))?;
        let path = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|entry| String::from_utf8_lossy(&entry.path).replace('\\', "/"));
        if path.as_deref() == Some(target.as_str()) {
            return Ok(Some(conflict));
        }
    }
    Ok(None)
}

fn blob_text(repo: &git2::Repository, oid: git2::Oid) -> Result<String, String> {
    let blob = repo
        .find_blob(oid)
        .map_err(|e| format!("Find blob: {}", e))?;
    Ok(String::from_utf8_lossy(blob.content()).to_string())
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
        .map(workspace::shell_compatible_path)
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
            .any(|entry| entry.path == "untracked.txt"
                && entry.status == "untracked"
                && !entry.staged));
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "staged.txt" && entry.status == "added" && entry.staged));
    }

    #[test]
    fn git_credential_secret_round_trips_username_and_password() {
        let secret = serialize_git_credentials("alice", "token-123");

        let parsed = parse_git_credentials_secret(&secret, None).unwrap();

        assert_eq!(parsed, ("alice".to_string(), "token-123".to_string()));
    }

    #[test]
    fn git_credential_secret_can_use_url_username() {
        let secret = serialize_git_credentials("", "token-123");

        let parsed = parse_git_credentials_secret(&secret, Some("git")).unwrap();

        assert_eq!(parsed, ("git".to_string(), "token-123".to_string()));
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
    fn git_status_accepts_windows_verbatim_workspace_path() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        let verbatim = format!("\\\\?\\{}", env.root.to_string_lossy());

        let status = git_status(verbatim).unwrap();

        assert_eq!(status.branch, "master");
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
    fn git_checkout_branch_can_create_and_switch_branch() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();

        git_checkout_branch(
            env.root.to_string_lossy().to_string(),
            "feature/demo".to_string(),
            true,
        )
        .unwrap();
        let status = git_status(env.root.to_string_lossy().to_string()).unwrap();

        assert_eq!(status.branch, "feature/demo");
        assert!(status
            .branches
            .iter()
            .any(|branch| branch.name == "feature/demo" && branch.current));
    }

    #[test]
    fn git_status_reports_conflicted_files() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        let repo = git2::Repository::open(&env.root).unwrap();
        let ancestor = repo.head().unwrap().peel_to_commit().unwrap();
        let sig = git2::Signature::now("Agent IDE Test", "agent@example.com").unwrap();

        git_checkout_branch(
            env.root.to_string_lossy().to_string(),
            "other".to_string(),
            true,
        )
        .unwrap();
        std::fs::write(env.root.join("tracked.txt"), "other\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "other", &tree, &[&ancestor])
            .unwrap();

        git_checkout_branch(
            env.root.to_string_lossy().to_string(),
            "master".to_string(),
            false,
        )
        .unwrap();
        std::fs::write(env.root.join("tracked.txt"), "master\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let master_parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "master", &tree, &[&master_parent])
            .unwrap();

        let other_ref = repo
            .find_branch("other", git2::BranchType::Local)
            .unwrap()
            .get()
            .resolve()
            .unwrap();
        let other = repo.reference_to_annotated_commit(&other_ref).unwrap();
        let mut opts = git2::MergeOptions::new();
        repo.merge(&[&other], Some(&mut opts), None).unwrap();
        let status = git_status(env.root.to_string_lossy().to_string()).unwrap();

        assert!(status.conflicts.iter().any(|path| path == "tracked.txt"));
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "tracked.txt" && entry.status == "conflicted"));
    }

    #[test]
    fn git_resolve_conflict_accepts_incoming_side() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        create_tracked_conflict(&env);

        git_resolve_conflict(
            env.root.to_string_lossy().to_string(),
            "tracked.txt".to_string(),
            "incoming".to_string(),
        )
        .unwrap();
        let status = git_status(env.root.to_string_lossy().to_string()).unwrap();
        let content = std::fs::read_to_string(env.root.join("tracked.txt")).unwrap();

        assert_eq!(content, "other\n");
        assert!(status.conflicts.is_empty());
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.path == "tracked.txt" && entry.staged));
    }

    #[test]
    fn git_checkout_remote_branch_creates_tracking_branch() {
        let _guard = workspace::env_test_guard();
        let env = TestRepo::new();
        let repo = git2::Repository::open(&env.root).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.reference(
            "refs/remotes/origin/feature",
            head.id(),
            true,
            "test remote",
        )
        .unwrap();

        git_checkout_remote_branch(
            env.root.to_string_lossy().to_string(),
            "origin/feature".to_string(),
            None,
        )
        .unwrap();
        let status = git_status(env.root.to_string_lossy().to_string()).unwrap();

        assert_eq!(status.branch, "feature");
        assert_eq!(
            status.upstream.as_deref(),
            Some("refs/remotes/origin/feature")
        );
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

    fn create_tracked_conflict(env: &TestRepo) {
        let repo = git2::Repository::open(&env.root).unwrap();
        let ancestor = repo.head().unwrap().peel_to_commit().unwrap();
        let sig = git2::Signature::now("Agent IDE Test", "agent@example.com").unwrap();

        git_checkout_branch(
            env.root.to_string_lossy().to_string(),
            "other".to_string(),
            true,
        )
        .unwrap();
        std::fs::write(env.root.join("tracked.txt"), "other\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "other", &tree, &[&ancestor])
            .unwrap();

        git_checkout_branch(
            env.root.to_string_lossy().to_string(),
            "master".to_string(),
            false,
        )
        .unwrap();
        std::fs::write(env.root.join("tracked.txt"), "master\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let master_parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "master", &tree, &[&master_parent])
            .unwrap();

        let other_ref = repo
            .find_branch("other", git2::BranchType::Local)
            .unwrap()
            .get()
            .resolve()
            .unwrap();
        let other = repo.reference_to_annotated_commit(&other_ref).unwrap();
        let mut opts = git2::MergeOptions::new();
        repo.merge(&[&other], Some(&mut opts), None).unwrap();
    }
}
