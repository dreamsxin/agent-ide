//! 把一件子任务交给一个**只读**的子 Agent 去做：它的提示词、边界和交回来的东西。
//!
//! 为什么要有它：主 Agent 的上下文是有限的，而"把这个仓库里所有用到 X 的地方找出来"这种活
//! 会把几十个文件的内容塞进主对话，然后挤掉真正要干的事。交给一个子 Agent 去翻，回来只带
//! 一段结论 —— 主 Agent 的上下文里只多了那一段。这是上下文预算上的杠杆，也是它能做完更大
//! 任务的原因。
//!
//! 先只有纯函数：提示词、边界、交回来的形状，都能单独测。真正的运行循环（第二部分）要把
//! `LlmClient` 接到工具那一侧，那是一次签名改动，单独一轮做。
//!
//! 参考实现（ZCode 的 `Agent`/`Task`）里有三点照搬、一点明确不照搬：
//! - 照搬：子 Agent **从零开始**（提示词必须自带全部背景，不继承父对话）、**不能再派子 Agent**
//!   （递归深度恰好 1）、交回来的只有**最后那段文字**，不是整个转录。
//! - 照搬：告诉它"不要写 report.md，父 Agent 读的是你的文字，不是你建的文件" —— 否则子 Agent
//!   最爱做的事就是在仓库里留一堆没人要的总结文件。
//! - **不照搬**：它的 `maxTurns` 一路传下去却**没有任何地方读**（默认值 4 看起来是个上限，
//!   其实不是），真正的边界只有一个"静默超时"。这里的轮次上限是真的会停。

/// 子 Agent 最多跑几轮工具调用。
///
/// 真的会停，不是一个摆设：一个读不到想要东西的子 Agent 会一直翻下去，而它花的每一分钱都记在
/// 用户账上。八轮够一次"找遍这个仓库"，不够的话它该带着"我看到这里"回来，让主 Agent 决定
/// 下一步 —— 那比它自己越翻越远要好。
pub const MAX_SUBAGENT_ROUNDS: usize = 8;

/// 交回父 Agent 的文字上限。
///
/// 这个杠杆的全部意义是"主上下文里只多一段结论"。子 Agent 回来倒三万字，等于把它本该替主
/// Agent 省下的上下文又原样还回去。
pub const MAX_SUBAGENT_RESULT_CHARS: usize = 20_000;

/// 一次委派的任务描述最长多少字符（给界面和日志用的短标题）
pub const MAX_SUBAGENT_DESCRIPTION_CHARS: usize = 60;

/// 提示词太短就不值得开一个子 Agent。
///
/// "看一下代码"这种委派换来的是一个瞎猜的子 Agent，它没有父 Agent 脑子里的那些背景 ——
/// 而它拿不到那些背景，因为它从零开始。
const MIN_SUBAGENT_PROMPT_CHARS: usize = 30;

/// 子 Agent 的系统提示词。
///
/// 每一条都对应一种实际会发生的糟糕行为：
/// - **只读**：它没有写工具，但提示词也要说 —— 说了它就不会把"我需要改这个文件"当成失败，
///   而是把结论写在回答里。
/// - **不要建文件**：不说的话它最爱做的就是留一个 `findings.md`，而那是仓库里没人要的垃圾，
///   父 Agent 也读不到（父 Agent 读的是它的文字）。
/// - **给路径和行号**：父 Agent 要接着干活，"在某个 service 里"这种回答等于让它自己再找一遍。
/// - **自带背景地回答**：它的回答会被塞进父 Agent 的上下文，而父 Agent 没看过它翻的过程。
/// - **不要问用户**：它没有提问的通道，问了就是白等，最后带回一句"我在等你回答"。
pub fn subagent_system_prompt() -> &'static str {
    "You are a research subagent working for another agent, not for a human. You can only read: \
     there are no write, command or network tools in this run, and that is deliberate.\n\n\
     Rules:\n\
     - Answer with text. Never create or edit files — not even a summary or a report file. The \
     agent that called you reads your message, not files you leave behind.\n\
     - Cite concrete locations as `path:line` so the caller can act without searching again.\n\
     - Your reply is the only thing that survives: the caller cannot see the files you read or the \
     searches you ran. Write it so it stands on its own.\n\
     - If you could not find something, say what you searched and what you ruled out. A precise \
     'not there, I looked in X and Y' is a useful answer; a guess is not.\n\
     - You cannot ask the user anything and you cannot delegate further. Decide with what you can \
     read.\n\
     - Stop as soon as you can answer. You have a limited number of tool rounds; spending them all \
     when the answer was clear after two wastes the caller's money."
}

/// 拼子 Agent 的任务提示词。
///
/// 项目上下文（`AGENTS.md` 等）跟着一起给：子 Agent 不继承父对话，但"这个项目的规矩"不属于
/// 对话，它属于这个仓库 —— 不给的话子 Agent 会按通用习惯去理解一个有自己约定的代码库。
pub fn subagent_user_prompt(goal: &str, project_context: Option<&str>) -> String {
    match project_context {
        Some(context) if !context.trim().is_empty() => format!(
            "{}\n\nProject context (for how this repository works, not part of the task):\n{}",
            goal.trim(),
            context.trim()
        ),
        _ => goal.trim().to_string(),
    }
}

/// 一次委派请求能不能受理。
///
/// 描述和提示词分开要求：描述是给人看的短标题（日志里一行），提示词是给子 Agent 的全部背景。
/// 两者都空的委派会变成一次没人知道在干什么、而子 Agent 也不知道要干什么的花费。
pub fn validate_request(description: &str, prompt: &str) -> Result<String, String> {
    let description = description.trim();
    let prompt = prompt.trim();
    if description.is_empty() {
        return Err(
            "A delegated task needs a short description so the run log says what it was."
                .to_string(),
        );
    }
    if prompt.chars().count() < MIN_SUBAGENT_PROMPT_CHARS {
        return Err(format!(
            "That prompt is too short ({} characters) to delegate. A subagent starts with no \
             knowledge of this conversation, so the prompt must carry the whole question — what to \
             look for, where you think it lives, and what a good answer contains.",
            prompt.chars().count()
        ));
    }
    let description = if description.chars().count() > MAX_SUBAGENT_DESCRIPTION_CHARS {
        description
            .chars()
            .take(MAX_SUBAGENT_DESCRIPTION_CHARS)
            .collect()
    } else {
        description.to_string()
    };
    Ok(description)
}

/// 子 Agent 交回来的东西。
pub struct SubagentResult {
    /// 最后那段文字（已按上限截断）
    pub text: String,
    pub truncated: bool,
    /// 实际用掉几轮工具调用
    pub rounds_used: usize,
    /// 是被轮次上限截停的，而不是自己答完了
    pub hit_round_cap: bool,
}

/// 把子 Agent 的最终文字收成可以交给父 Agent 的形状。
pub fn bound_result(text: &str, rounds_used: usize, hit_round_cap: bool) -> SubagentResult {
    let trimmed = text.trim();
    let (text, truncated) = if trimmed.chars().count() > MAX_SUBAGENT_RESULT_CHARS {
        (
            trimmed
                .chars()
                .take(MAX_SUBAGENT_RESULT_CHARS)
                .collect::<String>(),
            true,
        )
    } else {
        (trimmed.to_string(), false)
    };
    SubagentResult {
        text,
        truncated,
        rounds_used,
        hit_round_cap,
    }
}

/// 交给父 Agent 的那段话。
///
/// 除了结论本身，还要说清两件可能让结论不完整的事：**被截断了**，以及**是被轮次上限拦下来的**。
/// 不说的话，父 Agent 会把一个"翻到一半"的结论当成"翻完了"，然后据此下结论 —— 那比没有结论
/// 更糟，因为它看起来是有根据的。
pub fn format_for_caller(result: &SubagentResult) -> String {
    let mut out = if result.text.is_empty() {
        "The subagent returned nothing. Treat this as no answer, not as an empty result."
            .to_string()
    } else {
        result.text.clone()
    };
    let mut notes: Vec<String> = Vec::new();
    if result.hit_round_cap {
        notes.push(format!(
            "It stopped at the {}-round limit rather than finishing, so this answer may be \
             incomplete — delegate a narrower question if you need more",
            MAX_SUBAGENT_ROUNDS
        ));
    }
    if result.truncated {
        notes.push(format!(
            "Its reply was longer than {} characters and was cut",
            MAX_SUBAGENT_RESULT_CHARS
        ));
    }
    if !notes.is_empty() {
        out.push_str("\n\n[");
        out.push_str(&notes.join(". "));
        out.push_str(".]");
    }
    out
}

/// 给 action log 的一行：这次委派做了什么、花了几轮。
///
/// 花费只报这一次。委派是一次真实的开销（一个完整的模型循环），用户有权知道它发生过 ——
/// 但一行就够，不需要沿途的旁白。
pub fn delegation_log_line(description: &str, result: &SubagentResult) -> String {
    format!(
        "Delegated \"{}\": {} round(s), {} character(s) returned{}",
        description,
        result.rounds_used,
        result.text.chars().count(),
        if result.hit_round_cap {
            ", stopped at the round limit"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 系统提示词里那几条都对应一种实际会发生的糟糕行为，少一条就会看到它。
    #[test]
    fn the_subagent_prompt_forbids_what_subagents_actually_do() {
        let prompt = subagent_system_prompt();

        // 最常见的那一个：在仓库里留一个没人要的 findings.md
        assert!(prompt.contains("Never create or edit files"), "{}", prompt);
        assert!(prompt.contains("not files you leave behind"));
        // 父 Agent 要接着干活，所以要路径和行号
        assert!(prompt.contains("path:line"));
        // 它没有提问通道，问了就是白等
        assert!(prompt.contains("cannot ask the user"));
        // 递归深度 1
        assert!(prompt.contains("cannot delegate further"));
        // 答完就停，别把轮次花光
        assert!(prompt.contains("Stop as soon as you can answer"));
    }

    /// 子 Agent 从零开始，所以太短的提示词等于让它瞎猜。
    #[test]
    fn a_delegation_needs_a_self_contained_prompt_and_a_name() {
        assert!(validate_request("", "a fully formed question about the parser layout").is_err());
        let refusal = validate_request("Find X", "look at the code").unwrap_err();
        assert!(refusal.contains("too short"), "{}", refusal);
        assert!(refusal.contains("starts with no knowledge"), "{}", refusal);

        assert_eq!(
            validate_request(
                "  Map the parser  ",
                "Find every place the tokenizer is constructed and report the file:line of each."
            )
            .unwrap(),
            "Map the parser"
        );
        // 超长描述截断而不是拒绝：那只是日志里的一行标题，不值得为它失败一次委派
        let long = validate_request(&"x".repeat(200), &"y".repeat(60)).unwrap();
        assert_eq!(long.chars().count(), MAX_SUBAGENT_DESCRIPTION_CHARS);
    }

    /// 项目上下文要跟着给：它属于这个仓库，不属于父对话。
    #[test]
    fn the_task_prompt_carries_project_context_but_not_the_conversation() {
        let with = subagent_user_prompt("Find the tokenizer", Some("Always run cargo test."));
        assert!(with.starts_with("Find the tokenizer"));
        assert!(with.contains("Always run cargo test."));
        assert!(with.contains("not part of the task"));

        let without = subagent_user_prompt("Find the tokenizer", Some("   "));
        assert_eq!(without, "Find the tokenizer");
        assert_eq!(subagent_user_prompt("Find it", None), "Find it");
    }

    /// "翻到一半"绝不能看起来像"翻完了"。
    ///
    /// 父 Agent 看不到子 Agent 翻过什么，所以一个被轮次上限截停的结论如果不说明，它就会被
    /// 当成完整结论用下去 —— 那比没有结论更糟，因为它看起来有根据。
    #[test]
    fn an_incomplete_answer_says_it_is_incomplete() {
        let complete = bound_result("Found it at src/a.rs:12", 2, false);
        assert_eq!(format_for_caller(&complete), "Found it at src/a.rs:12");

        let capped = bound_result("Found two of them so far", MAX_SUBAGENT_ROUNDS, true);
        let text = format_for_caller(&capped);
        assert!(text.contains("stopped at the 8-round limit"), "{}", text);
        assert!(text.contains("may be incomplete"), "{}", text);

        let long = bound_result(&"x".repeat(MAX_SUBAGENT_RESULT_CHARS + 50), 3, false);
        assert!(long.truncated);
        assert!(format_for_caller(&long).contains("was cut"));

        // 什么都没返回要说成"没有答案"，不是"空结果"
        let empty = bound_result("   ", 1, false);
        let text = format_for_caller(&empty);
        assert!(text.contains("no answer"), "{}", text);
    }

    /// 花费报一行就够：委派是真实开销，用户有权知道，但不需要沿途旁白。
    #[test]
    fn the_log_line_says_what_it_cost_in_one_line() {
        let result = bound_result("answer", 3, true);
        let line = delegation_log_line("Map the parser", &result);

        assert!(line.contains("Map the parser"));
        assert!(line.contains("3 round(s)"));
        assert!(line.contains("stopped at the round limit"));
        assert_eq!(line.lines().count(), 1);
    }
}
