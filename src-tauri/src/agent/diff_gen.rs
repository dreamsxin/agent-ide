/// Diff 生成工具：基于 original 和 modified 文本生成 unified diff
/// 使用 similar crate 进行文本差异计算
pub fn generate_diff(original: &str, modified: &str, file_path: &str) -> String {
    let diff = similar::TextDiff::from_lines(original, modified);
    let mut output = String::new();

    output.push_str(&format!("--- a/{}\n", file_path));
    output.push_str(&format!("+++ b/{}\n", file_path));

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Equal => " ",
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
        };
        output.push_str(&format!("{}{}", sign, change.value()));
    }

    output
}

/// 计算两个文本之间的变更统计
pub fn diff_stats(original: &str, modified: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;

    let diff = similar::TextDiff::from_lines(original, modified);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Delete => deletions += 1,
            _ => {}
        }
    }

    (additions, deletions)
}
