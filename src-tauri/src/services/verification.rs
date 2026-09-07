//! 验证循环的共享部分：跑完检查命令之后，把失败变成一段可以直接发给模型的修复提示。
//!
//! 这些逻辑原本只存在于 `cli/mod.rs` 私有函数里，桌面端完全没有对应能力 ——
//! 桌面端能生成代码，但没法"跑一遍测试、失败了再修"。把它们提到服务层，两个
//! 入口共用同一套措辞和同一套截断规则，而不是各写一份然后慢慢漂移。

use crate::services::problem_parser::ProblemEntry;
use crate::services::project_tasks::RunProjectTaskResult;
use serde::Serialize;

/// 单条失败输出进提示词的字符上限
const MAX_COMMAND_OUTPUT_CHARS: usize = 16_000;
/// 提示词里列出的 problem 条数上限
const MAX_LISTED_PROBLEMS: usize = 40;

/// 退出码非 0 的检查。
///
/// 拿不到退出码（`None`）也算失败：命令没能正常结束，不该被当成通过。
pub fn failed_command_results(
    command_results: &[RunProjectTaskResult],
) -> Vec<RunProjectTaskResult> {
    command_results
        .iter()
        .filter(|result| result.exit_code.unwrap_or(-1) != 0)
        .cloned()
        .collect()
}

/// 各条命令解析出来的 problem，按 id 去重
pub fn collect_command_problems(command_results: &[RunProjectTaskResult]) -> Vec<ProblemEntry> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for problem in command_results
        .iter()
        .flat_map(|result| result.problems.clone())
    {
        if seen.insert(problem.id.clone()) {
            problems.push(problem);
        }
    }
    problems
}

pub fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n... truncated ...");
    truncated
}

/// 把失败的检查拼成一段修复提示。
///
/// 刻意只给失败项：把通过的检查也塞进去会稀释信号，模型容易开始"顺手改改"
/// 无关代码。同理提示里明确要求只返回可审查的 diff。
pub fn build_repair_prompt(
    original_prompt: &str,
    iteration: u8,
    command_results: &[RunProjectTaskResult],
    problems: &[ProblemEntry],
) -> String {
    let mut lines = vec![
        format!("Repair iteration {} for the original task.", iteration),
        "Original task:".to_string(),
        original_prompt.to_string(),
        String::new(),
        "Checks failed after applying the generated changes. Fix only the failures below."
            .to_string(),
        "Return reviewable Agent IDE diffs only.".to_string(),
        String::new(),
        "Parsed Problems:".to_string(),
    ];

    if problems.is_empty() {
        lines.push("(none parsed)".to_string());
    } else {
        for problem in problems.iter().take(MAX_LISTED_PROBLEMS) {
            lines.push(format!(
                "- {}:{}:{} [{}] {}: {}",
                problem.file,
                problem.line,
                problem.column,
                problem.severity,
                problem.source,
                problem.message
            ));
        }
        if problems.len() > MAX_LISTED_PROBLEMS {
            lines.push(format!(
                "... {} more problem(s) omitted",
                problems.len() - MAX_LISTED_PROBLEMS
            ));
        }
    }

    lines.push(String::new());
    lines.push("Failed command output:".to_string());
    for result in command_results
        .iter()
        .filter(|result| result.exit_code.unwrap_or(-1) != 0)
    {
        let output = [result.stdout.as_str(), result.stderr.as_str()]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        lines.push(format!(
            "\n$ {} (exit {})\n```text\n{}\n```",
            result.command,
            result
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            truncate_for_prompt(&output, MAX_COMMAND_OUTPUT_CHARS)
        ));
    }

    lines.join("\n")
}

/// 一次验证的结论，回给前端。
#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
    pub results: Vec<RunProjectTaskResult>,
    pub problems: Vec<ProblemEntry>,
    /// 失败的检查条数
    pub failed: usize,
    /// 全部通过时为 None —— 没有失败就没有要修的东西，
    /// 返回一段"修复提示"只会诱导用户白跑一轮
    pub repair_prompt: Option<String>,
}

/// 从已经跑完的检查结果生成结论
pub fn summarize(original_prompt: &str, results: Vec<RunProjectTaskResult>) -> VerificationReport {
    let failures = failed_command_results(&results);
    let problems = collect_command_problems(&results);
    let repair_prompt = if failures.is_empty() {
        None
    } else {
        Some(build_repair_prompt(original_prompt, 1, &results, &problems))
    };
    VerificationReport {
        failed: failures.len(),
        results,
        problems,
        repair_prompt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(command: &str, exit_code: Option<i32>, stderr: &str) -> RunProjectTaskResult {
        RunProjectTaskResult {
            command: command.to_string(),
            exit_code,
            stdout: String::new(),
            stderr: stderr.to_string(),
            problems: Vec::new(),
            duration_ms: 0,
        }
    }

    /// 拿不到退出码不能算通过：命令没能正常结束本身就是失败
    #[test]
    fn missing_exit_code_counts_as_failure() {
        let results = vec![
            result("npm test", Some(0), ""),
            result("cargo test", None, "crashed"),
            result("npm run lint", Some(1), "2 errors"),
        ];

        let failures = failed_command_results(&results);

        assert_eq!(failures.len(), 2);
        assert!(failures.iter().all(|item| item.command != "npm test"));
    }

    /// 全部通过时不该给出修复提示，否则会诱导用户白跑一轮修复
    #[test]
    fn passing_checks_produce_no_repair_prompt() {
        let report = summarize("add pagination", vec![result("npm test", Some(0), "")]);

        assert_eq!(report.failed, 0);
        assert!(report.repair_prompt.is_none());
    }

    #[test]
    fn repair_prompt_carries_only_the_failures() {
        let report = summarize(
            "add pagination",
            vec![
                result("npm test", Some(0), "all good"),
                result("npm run lint", Some(1), "Unexpected any"),
            ],
        );

        assert_eq!(report.failed, 1);
        let prompt = report.repair_prompt.expect("repair prompt");
        assert!(prompt.contains("add pagination"), "{}", prompt);
        assert!(prompt.contains("npm run lint"), "{}", prompt);
        assert!(prompt.contains("Unexpected any"), "{}", prompt);
        // 通过的检查不进提示词：混进去会稀释信号，模型容易顺手改无关代码
        assert!(!prompt.contains("all good"), "{}", prompt);
    }

    #[test]
    fn long_output_is_truncated_on_char_boundaries() {
        let multibyte = "错误".repeat(20_000);

        let truncated = truncate_for_prompt(&multibyte, 100);

        assert!(truncated.ends_with("... truncated ..."));
        // 按字符截断而不是按字节，否则多字节输出会在中间被切坏
        assert_eq!(
            truncated.chars().count(),
            100 + "\n... truncated ...".chars().count()
        );
    }
}
