//! 验证循环的共享部分：跑完检查命令之后，把失败变成一段可以直接发给模型的修复提示。
//!
//! 这些逻辑原本只存在于 `cli/mod.rs` 私有函数里，桌面端完全没有对应能力 ——
//! 桌面端能生成代码，但没法"跑一遍测试、失败了再修"。把它们提到服务层，两个
//! 入口共用同一套措辞和同一套截断规则，而不是各写一份然后慢慢漂移。

use crate::services::problem_parser::ProblemEntry;
use crate::services::project_tasks::RunProjectTaskResult;
use serde::Serialize;
use std::path::PathBuf;

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

/// 截断命令输出时保**尾部**。
///
/// 编译错误、断言失败、堆栈几乎总在输出末尾，前面是装依赖、编译进度之类的噪音。
/// 原本保的是头部，长输出下等于把真正的失败原因整段丢掉、只把噪音喂给模型 ——
/// 这是把 Rust 和 TypeScript 两处实现摆在一起对比才看出来的，CLI 一直在用错的那一半。
pub fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let total = value.chars().count();
    if total <= max_chars {
        return value.to_string();
    }
    let omitted = total - max_chars;
    let tail: String = value.chars().skip(omitted).collect();
    format!("... {} earlier character(s) omitted ...\n{}", omitted, tail)
}

/// 命令是否在允许清单里。
///
/// 支持 `*`（全部放行）和前缀通配 `cargo *`。原本只在 `cli/mod.rs` 里私有，
/// 但 Agent 的验证工具需要同一套判定 —— 两个入口对"什么算被授权"的理解
/// 必须一致，否则 CLI 拦得住的命令桌面端可能放过去。
pub fn is_command_allowed(command: &str, allowed: &[String]) -> bool {
    let command = normalize_command_pattern(command);
    allowed.iter().any(|pattern| {
        let pattern = normalize_command_pattern(pattern);
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return command.starts_with(prefix.trim_end());
        }
        command == pattern
    })
}

pub fn normalize_command_pattern(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// 这条命令是否会长驻不退出。
///
/// 验证是"逐条跑完再看结果"，把 `npm run dev` 之类塞进去会永远卡住 —— 这不是
/// 假设，开发服务器本来就不该退出。所以这是安全不变量而不是偏好开关：任何
/// 验证批次都不该包含长驻命令，想跑它们请用命令面板。
/// 排除优先于包含，`build:watch` 这种要被排除掉。
pub fn is_long_running_command(command: &str) -> bool {
    const LONG_RUNNING_HINTS: [&str; 7] = [
        "dev",
        "start",
        "watch",
        "serve",
        "preview",
        "--watch",
        "tauri dev",
    ];
    let lowered = command.to_lowercase();
    if LONG_RUNNING_HINTS
        .iter()
        .any(|hint| lowered.split_whitespace().any(|word| word == *hint) || lowered.contains(hint))
    {
        return true;
    }
    // `cargo run` 启动的是这个项目的应用本身，对本仓库就是 Tauri 桌面端 ——
    // 它不会退出。它躲过了上面的关键词表，而 `run` 不能加进那张表：判定里有
    // `contains` 兜底，加了会把 `npm run test` 也一起挡掉。
    //
    // 只按前两个 token 判定，所以 `npm run x` 不受影响。发现方式：
    // `smoke ide-surface` 把 `cargo run` 列成了验证候选命令。
    let mut tokens = lowered.split_whitespace();
    matches!((tokens.next(), tokens.next()), (Some("cargo"), Some("run")))
}

/// 一次验证的结论，回给前端。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub results: Vec<RunProjectTaskResult>,
    pub problems: Vec<ProblemEntry>,
    /// 失败的检查条数
    pub failed: usize,
    /// 因为会长驻而没有执行的命令。列出来而不是静默丢弃：
    /// 用户需要知道这次验证实际覆盖了什么
    pub skipped: Vec<String>,
    /// 全部通过时为 None —— 没有失败就没有要修的东西，
    /// 返回一段"修复提示"只会诱导用户白跑一轮
    pub repair_prompt: Option<String>,
}

/// 从已经跑完的检查结果生成结论
pub fn summarize(
    original_prompt: &str,
    results: Vec<RunProjectTaskResult>,
    skipped: Vec<String>,
) -> VerificationReport {
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
        skipped,
        repair_prompt,
    }
}

/// 把请求里的候选命令整理成"要跑的"和"跳过的"两份。
///
/// 从 `commands::agent::verify_workspace` 抽出来的。留在命令体里意味着这段逻辑
/// 只能靠启动整个桌面应用才能验证，而它包含一条安全不变量：长驻命令一律挡掉。
/// 验证是逐条跑完再看结果，混进一个 `npm run dev` 就永远卡住。
///
/// 两种 Err 是分开的，因为原因不同：一种是调用方什么都没给，一种是给的全都长驻。
/// 合成一句话的话，用户不知道该补命令还是该换命令。
pub fn prepare_commands(requested: Vec<String>) -> Result<(Vec<String>, Vec<String>), String> {
    let commands: Vec<String> = requested
        .into_iter()
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
        .collect();
    if commands.is_empty() {
        return Err("No verification commands were provided.".to_string());
    }

    let (commands, skipped): (Vec<String>, Vec<String>) = commands
        .into_iter()
        .partition(|command| !is_long_running_command(command));
    if commands.is_empty() {
        return Err(format!(
            "Every candidate looks long-running, so nothing could be verified: {}",
            skipped.join(", ")
        ));
    }
    Ok((commands, skipped))
}

/// 逐条跑检查命令，收集结果。
///
/// 命令没能启动（可执行文件不存在之类）也记成一条失败的检查，而不是让整批中断：
/// 否则第一条起不来就会把前面已经跑完的结果一起丢掉，用户看到的是"验证失败"
/// 而不是"哪几条过了、哪一条根本没跑起来"。
pub async fn run_checks(commands: Vec<String>, root: PathBuf) -> Vec<RunProjectTaskResult> {
    let mut results = Vec::new();
    for command in commands {
        match crate::services::project_tasks::run_project_command(command.clone(), root.clone())
            .await
        {
            Ok(result) => results.push(result),
            Err(message) => results.push(RunProjectTaskResult {
                command,
                exit_code: None,
                stdout: String::new(),
                stderr: message,
                problems: Vec::new(),
                duration_ms: 0,
            }),
        }
    }
    results
}

/// 验证结果渲染成 action log 的三段：等级、一句话结论、逐条明细。
///
/// 等级由"有没有失败"决定，而不是由"有没有跳过"：跳过的命令已经在报告里列出，
/// 把它算成 warn 会让一次全过的验证看起来像出了问题。
pub fn format_report_log(report: &VerificationReport) -> (&'static str, String, String) {
    let level = if report.failed == 0 {
        "success"
    } else {
        "warn"
    };
    let summary = format!(
        "Verification: {} of {} check(s) failed",
        report.failed,
        report.results.len()
    );
    let details = report
        .results
        .iter()
        .map(|result| {
            format!(
                "$ {} -> exit {}",
                result.command,
                result
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    (level, summary, details)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 候选整理：空白项丢掉，长驻命令进 skipped 而不是进执行队列。
    #[test]
    fn prepare_commands_trims_blanks_and_skips_long_running() {
        let (commands, skipped) = prepare_commands(vec![
            "  cargo test  ".to_string(),
            "   ".to_string(),
            "npm run dev".to_string(),
            "npm test".to_string(),
        ])
        .unwrap();

        assert_eq!(commands, vec!["cargo test", "npm test"]);
        assert_eq!(skipped, vec!["npm run dev"]);
    }

    /// 两种失败必须说不同的话：什么都没给 vs 给的全是长驻命令。
    /// 合成一句的话，用户不知道该补命令还是该换命令。
    #[test]
    fn prepare_commands_separates_empty_from_all_skipped() {
        let empty = prepare_commands(vec!["  ".to_string()]).unwrap_err();
        assert!(empty.contains("No verification commands"), "{}", empty);

        let all_skipped =
            prepare_commands(vec!["npm run dev".to_string(), "cargo watch".to_string()])
                .unwrap_err();
        assert!(
            all_skipped.contains("long-running"),
            "应当点明是因为长驻而没跑: {}",
            all_skipped
        );
        // 跳过的命令要列出来，否则用户不知道是哪几条被判定成长驻的
        assert!(all_skipped.contains("npm run dev"), "{}", all_skipped);
    }

    /// 一条跑不起来的命令不能让整批中断：前面已经跑完的结果必须还在，
    /// 那条坏的记成一次失败的检查。
    ///
    /// 这里不断言 `exit_code == None`。命令是经 shell 执行的，"找不到可执行文件"
    /// 在不同平台上分两种结局：shell 自己报错并给出非零退出码（Windows 就是这样），
    /// 或者进程根本没起来、`run_project_command` 返回 Err。两条路都要落成"失败的
    /// 检查"，所以断言在这一层，而不是在退出码的具体形状上。
    #[test]
    fn run_checks_keeps_earlier_results_when_one_command_cannot_run() {
        let root = std::env::current_dir().unwrap();
        let results = tokio::runtime::Runtime::new().unwrap().block_on(run_checks(
            vec![
                "cargo --version".to_string(),
                "definitely-not-a-real-binary-9f2c".to_string(),
            ],
            root,
        ));

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].exit_code, Some(0));
        assert_eq!(results[0].command, "cargo --version");
        assert_eq!(results[1].command, "definitely-not-a-real-binary-9f2c");
        let failures = failed_command_results(&results);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].command, "definitely-not-a-real-binary-9f2c");
    }

    /// 全过是 success，有失败才是 warn。跳过的命令不影响等级：
    /// 报告里已经列了 skipped，把它算成 warn 会让一次全过的验证看起来出了问题。
    #[test]
    fn format_report_log_levels_track_failures_not_skips() {
        let passing = summarize(
            "task",
            vec![RunProjectTaskResult {
                command: "cargo test".to_string(),
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                problems: Vec::new(),
                duration_ms: 1,
            }],
            vec!["npm run dev".to_string()],
        );
        let (level, summary, details) = format_report_log(&passing);
        assert_eq!(level, "success");
        assert!(summary.contains("0 of 1"), "{}", summary);
        assert_eq!(details, "$ cargo test -> exit 0");

        let failing = summarize(
            "task",
            vec![RunProjectTaskResult {
                command: "cargo test".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: "could not launch".to_string(),
                problems: Vec::new(),
                duration_ms: 1,
            }],
            Vec::new(),
        );
        let (level, summary, details) = format_report_log(&failing);
        assert_eq!(level, "warn");
        assert!(summary.contains("1 of 1"), "{}", summary);
        // 没有退出码时写 unknown，而不是留空或假装是 0
        assert_eq!(details, "$ cargo test -> exit unknown");
    }

    /// 长驻命令必须挡在验证批次之外：验证是逐条跑完再看结果，
    /// 混进一个不退出的开发服务器就永远卡住。
    #[test]
    fn long_running_commands_are_recognized() {
        for command in [
            "npm run dev",
            "npm start",
            "npm run build:watch",
            "vite preview",
            "cargo watch -x test",
            "npm run tauri dev",
            // `cargo run` 启动这个项目的应用本身（本仓库是 Tauri 桌面端），不会退出。
            // 它躲过了关键词表，被 `smoke ide-surface` 列成验证候选才发现。
            "cargo run",
            "cargo run --release",
            "cargo run --bin agent_cli",
        ] {
            assert!(
                is_long_running_command(command),
                "{} should be long running",
                command
            );
        }

        for command in [
            "npm test",
            "npm run lint",
            "npm run build",
            "cargo test",
            "cargo clippy -- -D warnings",
            "tsc --noEmit",
            // `run` 不能进关键词表：判定有 `contains` 兜底，加了会把这些一起挡掉
            "npm run test",
            "npm run typecheck",
        ] {
            assert!(
                !is_long_running_command(command),
                "{} should be runnable",
                command
            );
        }
    }

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
        let report = summarize(
            "add pagination",
            vec![result("npm test", Some(0), "")],
            Vec::new(),
        );

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
            vec!["npm run dev".to_string()],
        );

        assert_eq!(report.failed, 1);
        // 跳过的命令要如实列出，用户才知道这次验证覆盖了什么
        assert_eq!(report.skipped, vec!["npm run dev".to_string()]);
        let prompt = report.repair_prompt.expect("repair prompt");
        assert!(prompt.contains("add pagination"), "{}", prompt);
        assert!(prompt.contains("npm run lint"), "{}", prompt);
        assert!(prompt.contains("Unexpected any"), "{}", prompt);
        // 通过的检查不进提示词：混进去会稀释信号，模型容易顺手改无关代码
        assert!(!prompt.contains("all good"), "{}", prompt);
    }

    /// 报错在末尾，所以截断要保尾巴。保头部等于把失败原因丢掉、只留噪音。
    #[test]
    fn truncation_keeps_the_tail_where_the_failures_are() {
        let noisy = format!("{}FINAL FAILURE LINE", "progress\n".repeat(4000));

        let truncated = truncate_for_prompt(&noisy, 100);

        assert!(truncated.ends_with("FINAL FAILURE LINE"), "{}", truncated);
        assert!(truncated.starts_with("... "), "{}", truncated);
    }

    #[test]
    fn truncation_cuts_on_char_boundaries() {
        let multibyte = "错误".repeat(20_000);

        let truncated = truncate_for_prompt(&multibyte, 100);

        // 按字符截断而不是按字节，否则多字节输出会在中间被切坏
        assert!(truncated.ends_with("错误"));
        assert!(truncated.chars().count() < 200);
    }

    #[test]
    fn short_output_is_left_alone() {
        assert_eq!(truncate_for_prompt("2 errors", 100), "2 errors");
    }
}
