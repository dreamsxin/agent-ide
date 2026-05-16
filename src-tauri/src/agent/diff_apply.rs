use crate::agent::state_machine::{ApplyDiffError, ApplyDiffsResult, FileDiff};
use crate::services::workspace;
use std::hash::{Hash, Hasher};

pub(crate) fn apply_diff_to_path(
    file_path: &std::path::Path,
    diff: &FileDiff,
) -> Result<bool, String> {
    use std::fs;

    let Some(updated_content) = build_updated_content(file_path, diff)? else {
        return Ok(false);
    };

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create dir failed: {}", e))?;
    }
    fs::write(file_path, updated_content)
        .map_err(|e| format!("Write {}: {}", file_path.display(), e))?;

    Ok(true)
}

fn build_updated_content(
    file_path: &std::path::Path,
    diff: &FileDiff,
) -> Result<Option<String>, String> {
    use std::fs;

    if diff.hunks.is_empty() {
        return Ok(None);
    }

    let is_new_file = diff
        .hunks
        .iter()
        .all(|hunk| hunk.original.is_empty() && !hunk.updated.is_empty());

    if is_new_file {
        if file_path.exists() {
            return Err(format!(
                "Refusing to overwrite existing file: {}",
                file_path.display()
            ));
        }
        return Ok(Some(
            diff.hunks
                .iter()
                .map(|hunk| hunk.updated.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }

    let mut content = fs::read_to_string(file_path)
        .map_err(|_| format!("File not found: {}", file_path.display()))?;
    validate_base_hash(&content, diff, file_path)?;

    for hunk in &diff.hunks {
        if hunk.original.is_empty() {
            return Err(format!(
                "Mixed new-file and edit hunks are not supported for {}",
                file_path.display()
            ));
        }
        content = replace_unique(&content, &hunk.original, &hunk.updated).map_err(|message| {
            format!(
                "{} in {}: {}",
                message,
                file_path.display(),
                hunk.original[..hunk.original.len().min(200)].replace('\n', "\\n")
            )
        })?;
    }

    Ok(Some(content))
}

fn validate_base_hash(
    content: &str,
    diff: &FileDiff,
    file_path: &std::path::Path,
) -> Result<(), String> {
    let Some(expected) = diff.base_hash.as_deref() else {
        return Ok(());
    };
    let actual = content_hash(content);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "File changed since diff was generated for {}: expected baseHash {}, got {}",
            file_path.display(),
            expected,
            actual
        ))
    }
}

pub fn content_hash(content: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn apply_pending_diffs(diffs: &[FileDiff]) -> ApplyDiffsResult {
    let mut applied: Vec<FileDiff> = Vec::new();
    let mut failed: Vec<ApplyDiffError> = Vec::new();

    for diff in diffs {
        if diff.status != "pending" {
            continue;
        }

        let file_path = match workspace::resolve_for_write(&diff.file) {
            Ok(path) => path,
            Err(err) => {
                failed.push(ApplyDiffError {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    message: err,
                });
                continue;
            }
        };

        match apply_diff_to_path(&file_path, diff) {
            Ok(true) => applied.push(diff.clone()),
            Ok(false) => {}
            Err(message) => failed.push(ApplyDiffError {
                diff_id: diff.id.clone(),
                file: diff.file.clone(),
                message,
            }),
        }
    }

    ApplyDiffsResult { applied, failed }
}

fn replace_unique(text: &str, original: &str, updated: &str) -> Result<String, String> {
    if original.is_empty() {
        return Err("Original content is empty".to_string());
    }

    let exact_count = text.matches(original).count();
    if exact_count == 1 {
        return Ok(text.replacen(original, updated, 1));
    }
    if exact_count > 1 {
        return Err("Original content matched more than once".to_string());
    }

    let orig_trim = original.trim();
    if orig_trim != original && !orig_trim.is_empty() {
        let trim_count = text.matches(orig_trim).count();
        if trim_count == 1 {
            return Ok(text.replacen(orig_trim, updated.trim(), 1));
        }
        if trim_count > 1 {
            return Err("Original content matched more than once".to_string());
        }
    }

    Err("Could not find original content".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state_machine::DiffHunk;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agent-ide-apply-diff-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = temp_dir();
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write_file(&self, relative: &str, content: &str) {
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

    fn make_diff(file: &str, original: &str, updated: &str) -> FileDiff {
        FileDiff {
            id: Uuid::new_v4().to_string(),
            file: file.to_string(),
            base_hash: None,
            provenance: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: original.lines().count().max(1) as u32,
                new_start: 1,
                new_lines: updated.lines().count().max(1) as u32,
                content: String::new(),
                original: original.to_string(),
                updated: updated.to_string(),
                provenance: None,
                status: None,
            }],
            status: "pending".to_string(),
        }
    }

    #[test]
    fn apply_diff_to_path_creates_new_file() {
        let dir = temp_dir();
        let path = dir.join("new-file.ts");
        let diff = make_diff("new-file.ts", "", "export const created = true;\n");

        let written = apply_diff_to_path(&path, &diff).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(written);
        assert_eq!(content, "export const created = true;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_updates_existing_file() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\nconsole.log(value);\n").unwrap();
        let diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");

        let written = apply_diff_to_path(&path, &diff).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(written);
        assert!(content.contains("const value = 2;"));
        assert!(!content.contains("const value = 1;"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_reports_missing_original() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\n").unwrap();
        let diff = make_diff("edit.ts", "const value = 9;", "const value = 2;");

        let err = apply_diff_to_path(&path, &diff).unwrap_err();

        assert!(err.contains("Could not find original content"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_pending_diffs_reports_partial_success() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/ok.ts", "const value = 1;\n");
        env.write_file("src/fail.ts", "const other = 1;\n");

        let ok_diff = make_diff("src/ok.ts", "const value = 1;", "const value = 2;");
        let fail_diff = make_diff("src/fail.ts", "const missing = 1;", "const value = 2;");

        let result = apply_pending_diffs(&[ok_diff.clone(), fail_diff.clone()]);

        assert_eq!(result.applied.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.applied[0].id, ok_diff.id);
        assert_eq!(result.failed[0].diff_id, fail_diff.id);
        assert!(result.failed[0]
            .message
            .contains("Could not find original content"));
        assert_eq!(
            std::fs::read_to_string(env.root.join("src/ok.ts")).unwrap(),
            "const value = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("src/fail.ts")).unwrap(),
            "const other = 1;\n"
        );
    }

    #[test]
    fn apply_diff_to_path_rejects_ambiguous_original_without_writing() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\nconst value = 1;\n").unwrap();
        let diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");

        let err = apply_diff_to_path(&path, &diff).unwrap_err();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(err.contains("matched more than once"));
        assert_eq!(content, "const value = 1;\nconst value = 1;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_rejects_new_file_overwrite() {
        let dir = temp_dir();
        let path = dir.join("existing.ts");
        std::fs::write(&path, "export const existing = true;\n").unwrap();
        let diff = make_diff("existing.ts", "", "export const created = true;\n");

        let err = apply_diff_to_path(&path, &diff).unwrap_err();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(err.contains("Refusing to overwrite"));
        assert_eq!(content, "export const existing = true;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_keeps_file_unchanged_when_later_hunk_fails() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const first = 1;\nconst second = 1;\n").unwrap();
        let mut diff = make_diff("edit.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const missing = 1;".to_string(),
            updated: "const missing = 2;".to_string(),
            provenance: None,
            status: None,
        });

        let err = apply_diff_to_path(&path, &diff).unwrap_err();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(err.contains("Could not find original content"));
        assert_eq!(content, "const first = 1;\nconst second = 1;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_rejects_stale_base_hash() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\n").unwrap();
        let mut diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        diff.base_hash = Some(content_hash("const value = 0;\n"));

        let err = apply_diff_to_path(&path, &diff).unwrap_err();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(err.contains("File changed since diff was generated"));
        assert_eq!(content, "const value = 1;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_diff_to_path_accepts_matching_base_hash() {
        let dir = temp_dir();
        let path = dir.join("edit.ts");
        std::fs::write(&path, "const value = 1;\n").unwrap();
        let mut diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        diff.base_hash = Some(content_hash("const value = 1;\n"));

        let written = apply_diff_to_path(&path, &diff).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(written);
        assert_eq!(content, "const value = 2;\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
