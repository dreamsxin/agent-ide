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

        let status_str = if status.is_index_new() || status.is_wt_new() {
            "added"
        } else if status.is_index_deleted() || status.is_wt_deleted() {
            "deleted"
        } else if status.is_index_modified() || status.is_wt_modified() {
            "modified"
        } else if status.is_index_renamed() || status.is_wt_renamed() {
            "renamed"
        } else if status.is_wt_new() {
            "untracked"
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
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;

    let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
    let tree = head.peel_to_tree().map_err(|e| format!("Tree: {}", e))?;

    let mut diff_opts = git2::DiffOptions::new();
    if let Some(ref f) = file {
        diff_opts.pathspec(f);
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
    let repo = git2::Repository::discover(&path).map_err(|e| format!("Not a git repo: {}", e))?;

    let signature = repo
        .signature()
        .map_err(|e| format!("No git user configured: {}", e))?;

    let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;

    // Stage files
    if let Some(file_list) = files {
        for file in &file_list {
            index
                .add_path(std::path::Path::new(file))
                .map_err(|e| format!("Add {}: {}", file, e))?;
        }
    } else {
        // Stage all
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| format!("Add all: {}", e))?;
    }

    index.write().map_err(|e| format!("Write index: {}", e))?;

    let oid = index.write_tree().map_err(|e| format!("Write tree: {}", e))?;
    let tree = repo.find_tree(oid).map_err(|e| format!("Find tree: {}", e))?;

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
