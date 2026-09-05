use crate::agent::state_machine::{ApplyDiffError, ApplyDiffsResult, FileDiff};
use crate::services::workspace;
use std::collections::HashSet;
use std::path::PathBuf;

/// 单文件应用的便捷入口，仅测试使用。
/// 生产路径一律走 `apply_pending_diffs`，因为它还要负责同批次内的写入追踪。
#[cfg(test)]
fn apply_diff_to_path(file_path: &std::path::Path, diff: &FileDiff) -> Result<bool, String> {
    apply_diff_to_path_inner(file_path, diff, false)
}

/// `skip_base_hash` 只在同一批 apply 中该文件已被我们自己写过时为 true：
/// 此时内容变化来自上一个 hunk/diff，而不是外部改动，baseHash 必然不匹配。
fn apply_diff_to_path_inner(
    file_path: &std::path::Path,
    diff: &FileDiff,
    skip_base_hash: bool,
) -> Result<bool, String> {
    use std::fs;

    let Some(updated_content) = build_updated_content(file_path, diff, skip_base_hash)? else {
        return Ok(false);
    };

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create dir failed: {}", e))?;
    }
    fs::write(file_path, updated_content)
        .map_err(|e| format!("Write {}: {}", file_path.display(), e))?;

    Ok(true)
}

/// 该 diff 是否会创建一个新文件。
///
/// 判定依据是 hunk 形态（全部 hunk 的 original 为空、updated 非空），
/// 与 `build_updated_content` 实际走的分支完全一致。刻意不看
/// `provenance.operation`：那个字段可能是 `"unknown"`（后端生成的 diff 就是），
/// 用它做权限判断会留下绕过口子。
pub fn is_new_file_diff(diff: &FileDiff) -> bool {
    !diff.hunks.is_empty()
        && diff
            .hunks
            .iter()
            .all(|hunk| hunk.original.is_empty() && !hunk.updated.is_empty())
}

fn build_updated_content(
    file_path: &std::path::Path,
    diff: &FileDiff,
    skip_base_hash: bool,
) -> Result<Option<String>, String> {
    use std::fs;

    if diff.hunks.is_empty() {
        return Ok(None);
    }

    if is_new_file_diff(diff) {
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
    if !skip_base_hash {
        validate_base_hash(&content, diff, file_path)?;
    }

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

/// 内容指纹，用于检测 diff 生成之后文件是否被改动。
///
/// 这里刻意手写 FNV-1a 而不是用 `DefaultHasher`：标准库明确不保证
/// `DefaultHasher` 的结果在 Rust 版本之间稳定，而这个哈希会随待审查的 diff
/// 一起按 workspace 持久化、重启后再校验。用不稳定的哈希意味着升级工具链
/// 之后所有已保存的 diff 都会被误判为 stale。
pub fn content_hash(content: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}

/// 读取目标文件当前内容的哈希；文件不存在或不可读时返回 None。
fn current_content_hash(file: &str) -> Option<String> {
    let path = workspace::resolve_for_write(file).ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    Some(content_hash(&content))
}

/// 在生成 diff 时记录目标文件的当前内容哈希，供 apply 时检测外部改动。
///
/// 模型在 `agent-changes` 里给出的 `baseHash` 一律被覆盖：模型无从知道
/// Agent IDE 的哈希函数，它填的值只能是猜的。只有后端算出的哈希可信。
/// 新建文件（目标不存在）保持 None——没有可比较的基准内容。
pub fn stamp_base_hashes(diffs: &mut [FileDiff]) {
    for diff in diffs.iter_mut() {
        diff.base_hash = current_content_hash(&diff.file);
    }
}

/// apply 之后刷新仍待审查的 diff 的 baseHash。
///
/// 逐 hunk 审查会让文件内容前进，后续 hunk 必须以新内容为基准，否则第二个
/// hunk 会被误判为 stale。只刷新我们刚成功写过的文件：写入失败的情况下
/// 原有的 stale 判定必须保留。
///
/// 只处理 `pending` / `partial`：`failed` 的 diff 应当走"基于当前文件重新生成"
/// 的流程，而不是被悄悄改基准。
pub fn restamp_applied_files(diffs: &mut [FileDiff], applied: &[FileDiff]) {
    let written: HashSet<&str> = applied.iter().map(|diff| diff.file.as_str()).collect();
    for diff in diffs.iter_mut() {
        let has_remaining_work = diff.status == "pending" || diff.status == "partial";
        if has_remaining_work && written.contains(diff.file.as_str()) {
            diff.base_hash = current_content_hash(&diff.file);
        }
    }
}

pub fn apply_pending_diffs(diffs: &[FileDiff]) -> ApplyDiffsResult {
    let mut applied: Vec<FileDiff> = Vec::new();
    let mut failed: Vec<ApplyDiffError> = Vec::new();
    // 本批次内已写过的文件：它们的内容变化来自我们自己，不该再按 baseHash 判 stale
    let mut written: HashSet<PathBuf> = HashSet::new();

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

        let skip_base_hash = written.contains(&file_path);
        match apply_diff_to_path_inner(&file_path, diff, skip_base_hash) {
            Ok(true) => {
                written.insert(file_path);
                applied.push(diff.clone());
            }
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

    #[test]
    fn is_new_file_diff_classifies_by_hunk_shape() {
        assert!(is_new_file_diff(&make_diff(
            "new.ts",
            "",
            "export const created = true;\n"
        )));
        assert!(!is_new_file_diff(&make_diff(
            "edit.ts",
            "const value = 1;",
            "const value = 2;"
        )));

        // provenance 说的是 create，但 hunk 形态是编辑：按实际写盘行为判定，
        // 否则权限检查会和真实行为脱节
        let mut mislabeled = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        mislabeled.provenance = Some(crate::agent::state_machine::DiffProvenance {
            protocol: "agent-changes".to_string(),
            operation: "create".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        });
        assert!(!is_new_file_diff(&mislabeled));

        let mut empty = make_diff("edit.ts", "", "");
        empty.hunks.clear();
        assert!(!is_new_file_diff(&empty));
    }

    /// 这些是 FNV-1a 64 的标准测试向量。它们被钉死是为了保证 baseHash 在
    /// Rust 版本升级后仍然一致——已持久化的待审查 diff 依赖这一点。
    #[test]
    fn content_hash_matches_pinned_fnv1a_vectors() {
        assert_eq!(content_hash(""), "cbf29ce484222325");
        assert_eq!(content_hash("a"), "af63dc4c8601ec8c");
        assert_eq!(content_hash("foobar"), "85944171f73967e8");
        assert_eq!(content_hash("abc").len(), 16);
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }

    #[test]
    fn stamp_base_hashes_replaces_model_supplied_values() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/edit.ts", "const value = 1;\n");

        let mut edit = make_diff("src/edit.ts", "const value = 1;", "const value = 2;");
        // 模型只能猜 baseHash，这个值必须被后端算出的值覆盖
        edit.base_hash = Some("model-invented-hash".to_string());
        let mut create = make_diff("src/brand-new.ts", "", "export const created = true;\n");
        create.base_hash = Some("also-invented".to_string());
        let mut diffs = vec![edit, create];

        stamp_base_hashes(&mut diffs);

        assert_eq!(
            diffs[0].base_hash.as_deref(),
            Some(content_hash("const value = 1;\n").as_str())
        );
        // 目标文件还不存在，没有可比较的基准内容
        assert!(diffs[1].base_hash.is_none());
    }

    #[test]
    fn apply_pending_diffs_allows_two_diffs_on_the_same_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/seq.ts", "const first = 1;\nconst second = 1;\n");

        let mut diffs = vec![
            make_diff("src/seq.ts", "const first = 1;", "const first = 2;"),
            make_diff("src/seq.ts", "const second = 1;", "const second = 2;"),
        ];
        stamp_base_hashes(&mut diffs);

        let result = apply_pending_diffs(&diffs);
        let content = std::fs::read_to_string(env.root.join("src/seq.ts")).unwrap();

        // 第二个 diff 的 baseHash 记录的是第一个写入之前的内容；
        // 同批次内的改动来自我们自己，不应被判为 stale
        assert_eq!(result.applied.len(), 2, "failed: {:?}", result.failed);
        assert!(result.failed.is_empty());
        assert_eq!(content, "const first = 2;\nconst second = 2;\n");
    }

    #[test]
    fn apply_pending_diffs_still_rejects_externally_changed_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/edit.ts", "const value = 1;\n");

        let mut diffs = vec![make_diff(
            "src/edit.ts",
            "const value = 1;",
            "const value = 2;",
        )];
        stamp_base_hashes(&mut diffs);
        // 生成之后、应用之前，文件被外部改动
        env.write_file("src/edit.ts", "const value = 1;\n// touched\n");

        let result = apply_pending_diffs(&diffs);

        assert!(result.applied.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0]
            .message
            .contains("File changed since diff was generated"));
    }

    #[test]
    fn restamp_applied_files_only_refreshes_written_files() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/written.ts", "const written = 1;\n");
        env.write_file("src/untouched.ts", "const untouched = 1;\n");

        let mut diffs = vec![
            make_diff("src/written.ts", "const written = 1;", "const written = 2;"),
            make_diff(
                "src/untouched.ts",
                "const untouched = 1;",
                "const untouched = 2;",
            ),
        ];
        stamp_base_hashes(&mut diffs);
        let stale_stamp = diffs[1].base_hash.clone();

        // 模拟 written.ts 刚被应用过一个 hunk，文件内容已前进
        env.write_file("src/written.ts", "const written = 2;\n");
        let applied = vec![diffs[0].clone()];
        restamp_applied_files(&mut diffs, &applied);

        assert_eq!(
            diffs[0].base_hash.as_deref(),
            Some(content_hash("const written = 2;\n").as_str())
        );
        assert_eq!(diffs[1].base_hash, stale_stamp);
    }
}
