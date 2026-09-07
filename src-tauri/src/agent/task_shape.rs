//! 按请求形状决定要不要展开完整的多阶段流水线。
//!
//! 起因是一次实测：「创建 hello.txt 内容为 world」跑完 Architect → Coder →
//! Tester → Reviewer 四段，花了 5 次 LLM 调用、11295 token，还附赠一个没人要的
//! 18 行 test_hello.py。固定流水线给一行任务配四个角色，产出就是四个角色份量的
//! 东西 —— 现代 agent 框架的做法是先直接做，卡住了才升级。
//!
//! 这里刻意只用启发式、不额外调一次 LLM 去分类：为省调用而先花一次调用是自相
//! 矛盾的。判错的代价也不对称 —— 误判成 Direct 只是少了几段复核（改动仍然要人工
//! 审查才落盘），误判成 Full 只是多花钱，所以宁可在拿不准时给 Full。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskShape {
    /// 单文件、无需探索的改动：直接进实现阶段
    Direct,
    /// 需要设计、跨文件或需要排查：走完整流水线
    Full,
}

/// 超过这个长度就当成有实质约束要消化，不再算"顺手一改"
const MAX_DIRECT_PROMPT_CHARS: usize = 200;

/// 出现任一词就说明任务不是单点改动。中英文都列，因为界面是双语的。
const BROAD_TASK_MARKERS: &[&str] = &[
    "refactor",
    "migrate",
    "migration",
    "architecture",
    "redesign",
    "investigate",
    "debug",
    "diagnose",
    "optimize",
    "optimise",
    "performance",
    "across",
    "every file",
    "all files",
    "entire",
    "codebase",
    "test suite",
    "重构",
    "迁移",
    "架构",
    "设计",
    "排查",
    "调试",
    "优化",
    "性能",
    "所有文件",
    "整个项目",
    "全项目",
    "测试套件",
];

pub fn classify(prompt: &str) -> TaskShape {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_DIRECT_PROMPT_CHARS {
        return TaskShape::Full;
    }
    let lowered = trimmed.to_lowercase();
    if BROAD_TASK_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return TaskShape::Full;
    }
    // 提到两个及以上文件名的请求几乎都要协调改动，不适合单阶段
    if count_file_mentions(trimmed) > 1 {
        return TaskShape::Full;
    }
    TaskShape::Direct
}

/// 数出 prompt 里像文件名的 token（`name.ext`，扩展名 1-8 个字母数字）。
///
/// 刻意宽松：目的不是精确识别路径，而是判断"提到了几个东西"。
fn count_file_mentions(prompt: &str) -> usize {
    prompt
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '，' | '(' | ')' | '"' | '\''))
        .filter(|token| looks_like_file_name(token))
        .count()
}

fn looks_like_file_name(token: &str) -> bool {
    let token = token.trim_end_matches(['.', '。', ':', '：', ';']);
    let Some((stem, ext)) = token.rsplit_once('.') else {
        return false;
    };
    if stem.is_empty() || ext.is_empty() || ext.chars().count() > 8 {
        return false;
    }
    // 扩展名必须含字母，否则 "bump to 1.2" 里的版本号会被当成文件名
    ext.chars().any(|ch| ch.is_ascii_alphabetic())
        && ext.chars().all(|ch| ch.is_ascii_alphanumeric())
        && stem
            .chars()
            .any(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_measured_regression_case_is_direct() {
        // 这就是花了 5 次调用、并且凭空生成 test_hello.py 的那个请求
        assert_eq!(classify("创建 hello.txt 内容为 world"), TaskShape::Direct);
        assert_eq!(
            classify("create hello.txt with content world"),
            TaskShape::Direct
        );
    }

    #[test]
    fn single_file_edits_are_direct() {
        assert_eq!(classify("fix the typo in README.md"), TaskShape::Direct);
        assert_eq!(classify("给 utils.ts 加一个导出"), TaskShape::Direct);
    }

    #[test]
    fn broad_requests_keep_the_full_pipeline() {
        assert_eq!(classify("refactor the auth middleware"), TaskShape::Full);
        assert_eq!(classify("重构一下 diff 应用逻辑"), TaskShape::Full);
        assert_eq!(classify("optimize startup performance"), TaskShape::Full);
        assert_eq!(classify("排查一下这个测试为什么挂"), TaskShape::Full);
    }

    #[test]
    fn multi_file_requests_keep_the_full_pipeline() {
        assert_eq!(
            classify("move the parser from a.ts to b.ts"),
            TaskShape::Full
        );
    }

    #[test]
    fn long_prompts_keep_the_full_pipeline() {
        // 长 prompt 往往带着一堆约束，单阶段消化不了
        let long = "add a field".repeat(40);
        assert!(long.chars().count() > MAX_DIRECT_PROMPT_CHARS);
        assert_eq!(classify(&long), TaskShape::Full);
    }

    #[test]
    fn empty_prompt_is_not_treated_as_trivial() {
        assert_eq!(classify("   "), TaskShape::Full);
    }

    #[test]
    fn file_name_detection_ignores_prose_punctuation() {
        assert_eq!(count_file_mentions("update hello.txt."), 1);
        assert_eq!(count_file_mentions("no files here"), 0);
        // 版本号之类不该算文件
        assert_eq!(count_file_mentions("bump to 1.2"), 0);
    }
}
