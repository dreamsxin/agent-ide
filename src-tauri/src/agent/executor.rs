use crate::agent::multi_agent::AgentRole;
use crate::agent::state_machine::{DiffHunkProvenance, DiffProvenance, FileDiff, SddArtifact};
use crate::services::context::estimate_tokens_for_text;
use crate::services::llm_client::{
    synthesize_agent_changes_block, ChatMessage, HistoryTrim, LlmClient, LlmStreamOutput,
    LlmToolCall,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc;

/// 将原生工具调用合并进响应文本：追加合成的 agent-changes 围栏块，
/// 使下游 parse_diffs 管线无需感知传输方式。
fn merge_tool_call_output(output: LlmStreamOutput) -> String {
    let mut content = output.content;
    if !output.tool_calls.is_empty() {
        if let Some(block) = synthesize_agent_changes_block(&output.tool_calls) {
            content.push_str(&block);
        }
    }
    content
}

/// 外部工具执行入口。当前唯一实现来源是 MCP server 发现的工具。
///
/// Agent 内置的 `emit_agent_changes` / `emit_sdd_draft` 不走这里：
/// 它们是输出协议，而不是需要回传结果的副作用调用。
#[async_trait]
pub trait ToolInvoker: Send + Sync {
    /// 该工具名是否由本 invoker 处理
    fn handles(&self, tool_name: &str) -> bool;

    /// 执行工具并返回回传给模型的文本结果
    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String>;

    /// 取走上一次 `invoke` 产生的图片。
    ///
    /// 为什么是单独一个取回口，而不是让 `invoke` 返回一个带图的结构：`invoke` 的返回值
    /// 是"给模型看的文本"，三个实现里只有一个可能产生图片，而改签名要动所有实现和所有
    /// 测试替身。这条通道和写入日志、外部动作日志是同一个形状 —— 工具把副产物存进
    /// 自己的日志，调用方排空一次。
    ///
    /// 默认实现返回空：MCP 工具的返回值只有文本。
    fn take_images(&self) -> Vec<crate::services::images::ImagePart> {
        Vec::new()
    }
}

/// 单次 LLM 调用中允许的最大工具回合数。
///
/// 从 4 提到 12：以前唯一的工具来源是 MCP，回合数少无所谓；现在内置了只读的
/// 工作区工具，"搜索 → 读几个文件 → 再改" 是常规流程，4 回合会在探索途中被
/// 截断，模型只能靠猜写 ORIGINAL 段。真正的成本闸门是 per-run token 上限
/// （`RunUsageMeter`），这个常量只是防死循环的兜底。
pub const MAX_TOOL_ITERATIONS: usize = 12;

/// 一个 stage 最多续写几次。
///
/// 每次续写都要把整段上下文连同那半截回答再发一遍：既是一次完整计费，又让这一次
/// 请求剩下的输出空间更小（输出上限是 `窗口 - prompt`）。所以这里比 ZCode 的 3 更保守，
/// 取 2 —— 够补完一个被切断的块，又不会在预算本来就紧的会话里滚成三四次全量请求。
const MAX_OUTPUT_CONTINUATIONS: usize = 2;

/// 让模型接着写下去的那句话。
///
/// 刻意不让它重新解释或从头再来：重来会把已经写好的块再写一遍，等于花两倍的钱
/// 拿同一份东西，而且很可能再一次被切在同一个地方。
const OUTPUT_CONTINUATION_PROMPT: &str =
    "Your previous answer was cut off by the output limit mid-block. Resume exactly where it \
     stopped — no apology, no recap, do not repeat any block you already finished. If the cut \
     happened inside an `agent-changes` block, continue that JSON from the exact character it \
     ended on so the block closes. Then keep going with the remaining files, one block per file, \
     and stop early rather than being cut off again.";

/// 从模型返回的工具调用中挑出需要真正执行的外部工具调用。
/// 内置输出协议工具（`emit_agent_changes` / `emit_sdd_draft`）不在其中。
fn select_external_calls(
    calls: &[LlmToolCall],
    invoker: Option<&dyn ToolInvoker>,
) -> Vec<LlmToolCall> {
    match invoker {
        Some(invoker) => calls
            .iter()
            .filter(|call| invoker.handles(&call.name))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// 带外部工具回合的 LLM 调用。
///
/// 每一轮：请求 → 若模型调用了外部工具则执行并把结果作为 `role: "tool"` 消息回传 → 继续请求。
/// 未启用 invoker、或模型没有调用外部工具时，行为与单次 `stream_chat_with_tools` 完全一致。
///
/// 返回值带上本轮之后新增的消息（assistant 的工具调用、`tool` 结果、最终回答）。
/// 以前这些消息是这个函数的局部变量，`return` 时一起丢掉，只剩扁平文本 ——
/// 于是工具到底返回了什么，出了这个函数就没人知道了。
async fn stream_with_tool_loop(
    llm: &LlmClient,
    mut messages: Vec<ChatMessage>,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
    // 这一趟最多跑几轮。主运行用 `MAX_TOOL_ITERATIONS`，子 Agent 用一个更小的上限 ——
    // 参考实现那边的 `maxTurns` 一路传下去却没人读，于是"上限"只是个装饰。
    max_iterations: usize,
) -> Result<StageOutcome, String> {
    let prompt_len = messages.len();
    let mut merged = String::new();
    // 真正执行过工具的轮数。循环自己数，因为只有这里知道 —— transcript 会被修剪。
    let mut tool_rounds = 0usize;
    // 还没送出去的图片。跨轮存在，因为一个请求装不下的那些要留到下一轮，而不是让模型
    // 回头再读一遍 —— 它们已经花过运行预算了。
    let mut pending_images: Vec<crate::services::images::ImagePart> = Vec::new();

    for iteration in 0..=max_iterations {
        // 每一轮都把之前所有消息重发一遍，所以要在发之前看它还装不装得下。窗口未知时
        // `prompt_token_budget()` 返回 None —— 那种情况下不动历史，让供应商去拒绝，
        // 而不是按一个猜出来的窗口丢掉模型刚读到的东西。
        if let Some(budget) = llm.prompt_token_budget() {
            if let Some(trim) = trim_tool_loop_history(&mut messages, prompt_len, budget) {
                llm.note_history_trim(trim);
            }
        }
        let output = match llm
            .stream_chat_with_tools(messages.clone(), cancel_flag.clone(), tx.clone())
            .await
        {
            Ok(output) => output,
            // 最后一轮什么都没回来时，不能把前几轮已经完成的工作一起丢掉：工具结果已经花过
            // 预算、可能还落过盘，而 `?` 会让整个阶段失败，`merged` 和整段 transcript（每一条
            // `role: "tool"` 的结果）一起消失 —— 界面上还留着流出来的那半段回答，紧跟一句
            // "没有内容"的报错。所以前面有产出就把这次失败当成循环的终点，连着原因一起交上去。
            //
            // 取消是例外，必须原样往上抛：调用方靠那句字面量把"用户按了 Stop"和真正的失败
            // 分开，在这里咽掉会让一次取消被记成正常收尾。
            Err(error) => {
                if merged.trim().is_empty() || error == crate::agent::orchestrator::CANCELLED_ERROR
                {
                    return Err(error);
                }
                let note = format!(
                    "\n\n[agent-ide] The last round returned nothing ({}). Stopping here with what \
                     the earlier rounds produced.\n",
                    error
                );
                llm.note_dropped_images(
                    pending_images.len(),
                    "the tool loop ended before they fit in a request",
                );
                merged.push_str(&note);
                let mut transcript = messages.split_off(prompt_len);
                transcript.push(ChatMessage::assistant(note));
                return Ok(StageOutcome {
                    text: merged,
                    transcript,
                    tool_rounds,
                    hit_round_cap: false,
                    output_continuations: 0,
                });
            }
        };

        // 图片只发一次：它已经在上面那次请求里了。留着的代价是复利式的 —— 一张 4 MiB 的
        // 图 base64 后约 5.3 MB，12 轮工具循环会把它重发 11 次（约 59 MB 出网），而
        // transcript 修剪只按 `content` 的字符数算账，图片计 0，于是它既挤掉真正有用的
        // 文本又永远不会被淘汰。模型在紧跟工具调用的那一轮看到图，之后靠文本继续。
        for message in messages.iter_mut() {
            message.images.clear();
        }

        let external = select_external_calls(&output.tool_calls, invoker);

        let is_last_iteration = iteration == max_iterations;
        if external.is_empty() || is_last_iteration {
            let hit_round_cap = is_last_iteration && !external.is_empty();
            let mut final_text = merge_tool_call_output(output);
            if hit_round_cap {
                final_text.push_str(&format!(
                    "\n\n[agent-ide] Tool loop stopped after {} rounds; remaining tool calls were not executed.\n",
                    max_iterations
                ));
            }
            // 循环到这里就结束了，留着的图片再也没有请求可搭。它们花过预算却没被看到，
            // 所以要走和其他图片降级同一条汇报路径，而不是安静消失。
            llm.note_dropped_images(
                pending_images.len(),
                "the tool loop ended before they fit in a request",
            );
            merged.push_str(&final_text);
            let mut transcript = messages.split_off(prompt_len);
            transcript.push(ChatMessage::assistant(final_text));
            return Ok(StageOutcome {
                text: merged,
                transcript,
                tool_rounds,
                hit_round_cap,
                output_continuations: 0,
            });
        }

        // 保留本轮文本输出，模型可能同时给出解释和工具调用。
        //
        // 输出协议的调用（`emit_agent_changes` / `emit_sdd_draft`）在中途的回合里也要
        // 立刻合成进文本：它们不由 invoker 执行，而 `merge_tool_call_output` 只在最后
        // 一轮跑 —— 于是"这一轮既提交了 diff 又调了一个工具"会把整段 diff 丢掉。
        merged.push_str(&merge_tool_call_output(LlmStreamOutput {
            content: output.content.clone(),
            tool_calls: output
                .tool_calls
                .iter()
                .filter(|call| !external.iter().any(|done| done.id == call.id))
                .cloned()
                .collect(),
            usage: None,
        }));
        // 只重放**会被回答**的调用。带 `tool_calls` 的 assistant 消息后面必须紧跟每一个
        // 调用的 `tool` 结果，而输出协议那几个不会有结果 —— 一并重放的话下一次请求会被
        // 供应商整体拒掉（"tool_calls must be followed by tool messages"），而这一轮
        // 已经做过的事全部作废。
        messages.push(ChatMessage::assistant_tool_calls(
            output.content.clone(),
            &external,
        ));
        // 这一轮确定要执行工具了，算一轮。放在执行之前：中途被 Stop 的那一轮工具也已经
        // 开始花钱了，报给调用方的"用了几轮"不该把它抹掉。
        tool_rounds += 1;

        let invoker = invoker.expect("external calls only collected when invoker is present");
        // 这一轮工具产出的图片。它们**不能**挂在 `role: "tool"` 消息上：OpenAI 的
        // chat/completions 只在 user 消息里接受 image 块，tool 消息的 content 只能是文本，
        // 挂上去会得到一个和图片无关的 400，而唯一启用了图片的模型族恰好就是这一家。
        for call in &external {
            if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("Agent task cancelled".to_string());
            }
            // 工具失败也回传给模型，让它自行降级而不是直接中断整个 stage
            let result = match invoker.invoke(&call.name, &call.arguments).await {
                Ok(result) => result,
                Err(error) => format!("Tool call failed: {}", error),
            };
            // 失败路径也要排空：不排的话，这次的图会挂到下一次工具调用上，模型看到的图
            // 和它问的问题就错位了 —— 那种错比没有图更难查。
            pending_images.extend(invoker.take_images());
            messages.push(ChatMessage::tool_result(call.id.clone(), result));
        }
        if !pending_images.is_empty() {
            // 图片单独一条 user 消息跟在工具结果后面，这是 OpenAI 兼容端点唯一接受的位置。
            //
            // 一轮里可以有好几次读图调用，所以这里还要按**单个请求**再卡一次：运行预算
            // 放得过的四张图，base64 之后能把请求体顶到 provider 的上限之上，换来一条和
            // 图片无关的 413。装不下的**留到下一轮**，不是丢掉：它们已经花过运行预算，
            // 让模型回头再读一遍会撞第二次预算，而那次拒绝给的建议它做不到。
            let total = pending_images.len();
            let (kept, deferred) =
                crate::services::images::fit_images_in_request(std::mem::take(&mut pending_images));
            messages.push(ChatMessage::user(round_image_note(kept.len(), total)).with_images(kept));
            pending_images = deferred;
        }
    }

    // 循环里每条路径都 return 了，这里只为满足类型检查
    let transcript = messages.split_off(prompt_len);
    Ok(StageOutcome {
        text: merged,
        transcript,
        tool_rounds,
        hit_round_cap: false,
        output_continuations: 0,
    })
}

/// 附图那条 user 消息的正文。
///
/// 措辞是这里唯一容易错的东西，所以它是个纯函数：`kept` 张已经附上，剩下的留到下一轮。
/// 必须说清**是哪些** —— 截断从尾部走，模型靠顺序把图对上工具调用，只说"少了 2 张"
/// 它无从判断少的是哪两张。也必须说清"别再读一遍"：重读会撞运行预算，而那条拒绝里
/// 给出的建议（少读几张）它做不到。
fn round_image_note(kept: usize, total: usize) -> String {
    if kept >= total {
        return format!("Attached {} image(s) from the tool call(s) above.", total);
    }
    format!(
        "Attached the first {} of {} image(s) from the tool call(s) above. The last {} did not fit in this request and will be attached in the next step — do not read them again.",
        kept,
        total,
        total - kept
    )
}

/// 执行步骤的系统提示词
const EXECUTOR_PROMPT: &str = r#"You are a precise coding assistant. Your task is to implement ONE specific coding step.

## Output Format
Provide the implementation for this step. For code changes, you MUST use this diff format:

```diff:path/to/file
<<<<<<< ORIGINAL
existing code to replace
=======
new replacement code
>>>>>>> UPDATED
```

For new files, use:

```new:path/to/file
file content here
```

## Rules
1. If a tool for reading or searching the workspace is available, use it to read the exact
   current text of any file you intend to edit before writing a diff. Never guess an ORIGINAL
   section — a mismatch makes the change unappliable.
2. If a tool for running the project's check commands is available, use it to see the real
   failure output before deciding what to change, and cite what it reported. Do not describe a
   check as passing unless you ran it.
3. If a tool for writing workspace files is available, prefer it over emitting a diff, and then
   run the checks again to confirm the change works. Read the file first: a write replaces the
   whole file rather than patching it. If no write tool is offered, the user has chosen to review
   changes before they land — emit diffs instead.
4. Output ONLY code and diffs — no explanations unless no code change is needed
5. Each diff block must have exactly one ORIGINAL and one UPDATED section
6. For edits: show EXACT original code that needs to be replaced
7. Be precise — copy the original code exactly as it appears

Respond now with the implementation."#;

/// 执行单个步骤：调用 LLM 生成代码变更
pub async fn execute_step(
    llm: &LlmClient,
    step: &str,
    context: &str,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<String, String> {
    let messages = vec![
        ChatMessage::system(EXECUTOR_PROMPT),
        ChatMessage::user(format!(
            "Step to execute: {}\n\nContext:\n{}\n\nProvide the implementation (code/diff only):",
            step, context
        )),
    ];

    stream_with_tool_loop(llm, messages, invoker, cancel_flag, tx, MAX_TOOL_ITERATIONS)
        .await
        .map(|outcome| outcome.text)
}

/// 跑一个只读子 Agent，返回它最后那段文字、用掉的轮数，以及是不是撞了轮数上限。
///
/// 三件事和主运行刻意不同：
/// - **它的流不进用户的聊天区。** 子 Agent 的过程是给调用方看的中间产物，混进主回答里只会让
///   用户读到两个声音交替说话。这里给它一个自己的通道并在后台排空 —— 不排空的话，通道满了
///   之后 `send` 会永远等下去。
/// - **轮次上限更小**（`MAX_SUBAGENT_ROUNDS`），而且是真的会停。
/// - **取消共用父运行那一个开关**：用户按 Stop 是要停掉整件事，不是停掉最外面那一层。
///
/// 轮数和"撞没撞上限"都取自循环自己的记账。以前是在这里数 transcript 里带 `tool_calls` 的
/// assistant 消息、再拿 `rounds >= 上限` 当截断判据，两头都错：最后一轮不执行工具，所以
/// "用满 8 轮然后正常作答"会被报成截断；而 `trim_tool_loop_history` 会删掉最老的那几组消息，
/// 于是真被截断的长运行反而数少了、读起来像正常收尾。
pub async fn run_subagent(
    llm: &LlmClient,
    system_prompt: &str,
    task_prompt: &str,
    invoker: &dyn ToolInvoker,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(String, usize, bool), String> {
    let messages = vec![
        ChatMessage::system(system_prompt.to_string()),
        ChatMessage::user(task_prompt.to_string()),
    ];
    let (tx, mut rx) = mpsc::channel::<String>(64);
    // 后台排空：这些片段不给任何人看，但不读走就会把子 Agent 卡死在 `send` 上
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let outcome = stream_with_tool_loop(
        llm,
        messages,
        Some(invoker),
        cancel_flag,
        tx,
        crate::agent::subagent::MAX_SUBAGENT_ROUNDS,
    )
    .await;
    drain.abort();
    let outcome = outcome?;
    Ok((outcome.text, outcome.tool_rounds, outcome.hit_round_cap))
}

/// 单条消息带进下一个 stage 时的内容上限
const MAX_CARRIED_MESSAGE_CHARS: usize = 6_000;
/// 整条线程带进下一个 stage 时的总字符上限
const MAX_CARRIED_THREAD_CHARS: usize = 24_000;

/// 一个 stage 跑完之后的产出。
///
/// `text` 是扁平文本，给人看、给 diff 解析用；`transcript` 是这个 stage 真实发生过的
/// assistant / tool 消息，按顺序排列，交给下一个 stage 当历史。
///
/// 分两个字段是因为它们丢失的东西不同：`text` 里从来没有工具返回值
/// （`merge_tool_call_output` 只取模型自己的文本），所以在此之前，下一个 stage 看不到
/// "跑了 npm test，输出是这些"，只能看到模型转述的版本 —— 而转述恰恰是不可信的那部分。
#[derive(Clone, Debug, Default)]
pub struct StageOutcome {
    pub text: String,
    pub transcript: Vec<ChatMessage>,
    /// 这一趟真正执行过工具的轮数。
    ///
    /// 由循环自己数，**不从 `transcript` 反推**：`trim_tool_loop_history` 会把最老的
    /// assistant/tool 组从 `messages` 里删掉，而 transcript 就是 `messages` 的尾巴 ——
    /// 数它等于"被裁掉的那几轮没跑过"，而那正是最长、最该报给调用方的那些运行。
    pub tool_rounds: usize,
    /// 撞上了轮数上限：最后一轮里还有没执行的工具调用。
    ///
    /// 只有循环自己知道这件事。最后一轮不执行工具，所以"用满 N 轮之后正常作答"和
    /// "第 N 轮还想调工具但被拦下"在消息上长得一样 —— 而对调用方来说前者是完整答案，
    /// 后者是一段被截断的探索。子 Agent 把这个标志转述给主 Agent（`format_for_caller`）。
    pub hit_round_cap: bool,
    /// 因为回答被输出预算切断而续写了几次。
    ///
    /// 每次续写都是一次完整计费的请求，所以这个数字必须能报给用户 ——
    /// "这次为什么贵了一倍"只能由它回答。
    pub output_continuations: usize,
}

/// 把上游 stage 的消息线程裁进预算。
///
/// 之前这里是 `join_prior_outputs`：把各阶段输出拼成一段 prose 塞进 user 消息。
/// 换成真实消息之后，工具结果能原样传下去，但两条硬约束必须守住：
///
/// 1. 带 `tool_calls` 的 assistant 消息和它后面的 `tool` 结果必须同进同出。供应商
///    要求两者配对，只留一半会让整个请求 400 —— 那不是"少点历史"，是这一 stage 直接失败。
/// 2. 丢掉了内容就要说出来。缺一段而不声明，模型会把节选当成完整历史，
///    并据此断言"前面已经验证过了"。
///
/// 预算本身仍然是必要的：这条线程不经过上下文预算（预算只裁 `context`），
/// 阶段一多就会无界增长，把真正有用的项目上下文挤出窗口。
pub fn bound_transcript(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    bound_transcript_with_limits(
        messages,
        MAX_CARRIED_MESSAGE_CHARS,
        MAX_CARRIED_THREAD_CHARS,
    )
}

/// 截断一条要带进下一个 stage 的消息：保留第一行，其余按尾部截断。
///
/// `truncate_for_prompt` 保尾部，这对结论和 diff 是对的（它们都在末尾）。但出处标签
/// `[Stage / role]` 由 orchestrator 加在**头部**，于是任何超过上限的阶段输出，第一个被
/// 丢掉的就是归属信息 —— 恰好是长输出最需要标签的时候。所以第一行单独保住。
fn truncate_carried_message(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    match value.split_once('\n') {
        Some((first, rest)) => {
            let first_len = first.chars().count();
            // 第一行本身就超预算时没什么可保的，退回统一的尾部截断
            if first_len + 1 >= max_chars {
                return crate::services::verification::truncate_for_prompt(value, max_chars);
            }
            let budget = max_chars - first_len - 1;
            format!(
                "{}\n{}",
                first,
                crate::services::verification::truncate_for_prompt(rest, budget)
            )
        }
        None => crate::services::verification::truncate_for_prompt(value, max_chars),
    }
}

fn bound_transcript_with_limits(
    messages: &[ChatMessage],
    max_message_chars: usize,
    max_thread_chars: usize,
) -> Vec<ChatMessage> {
    bound_transcript_report(messages, max_message_chars, max_thread_chars).0
}

/// 同 `bound_transcript_with_limits`，另外返回**丢掉了几组**。
///
/// 组数而不是消息条数：一次工具调用和它的结果是一组，那才是给用户看的"一次来回"，而按
/// 消息条数算还会把声明用的那条系统消息算进去（每次少报一条）。
fn bound_transcript_report(
    messages: &[ChatMessage],
    max_message_chars: usize,
    max_thread_chars: usize,
) -> (Vec<ChatMessage>, usize) {
    // 分组：`tool` 消息永远归到前一条 assistant 上，这样丢弃只会按组发生
    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    for message in messages {
        let mut trimmed = message.clone();
        trimmed.content = truncate_carried_message(&message.content, max_message_chars);
        match groups.last_mut() {
            Some(group) if message.role == "tool" => group.push(trimmed),
            // 开头就是 `tool` 说明它的 assistant 调用不在这段历史里。带上去会让供应商
            // 拒掉整个请求（tool 必须紧跟发起它的调用），所以直接丢掉 —— 少一条历史，
            // 而不是这一 stage 整体失败。当前没有入口会产生这种线程，这里是护栏。
            None if message.role == "tool" => continue,
            _ => groups.push(vec![trimmed]),
        }
    }

    let mut kept: Vec<Vec<ChatMessage>> = Vec::new();
    let mut used = 0usize;
    for group in groups.iter().rev() {
        // 按 `message_cost_chars` 算账：工具调用的参数（一次写文件的整份内容）也要计入，
        // 否则一组"空 content + 8 万字符参数"的写调用会被当成几乎不占地方，预算形同虚设
        let cost: usize = group.iter().map(message_cost_chars).sum();
        // 最近的一组即使超预算也保留：紧邻的上一步是下一个 stage 最依赖的东西，
        // 一条历史都不给比给一条超长历史更糟
        if !kept.is_empty() && used + cost > max_thread_chars {
            break;
        }
        used += cost;
        kept.push(group.clone());
    }
    kept.reverse();

    let omitted = groups.len() - kept.len();
    let mut result: Vec<ChatMessage> = Vec::new();
    if omitted > 0 {
        result.push(ChatMessage::system(format!(
            "[agent-ide] {} earlier exchange(s) were omitted to fit the context budget. Do not assume work you cannot see here was done; re-read a file if you need it again.",
            omitted
        )));
    }
    result.extend(kept.into_iter().flatten());
    (result, omitted)
}

/// 工具回合里，循环长出来的那段历史至少要留下的字符数。
///
/// 预算算下来是 0 的时候也不能真的裁到 0：`bound_transcript_with_limits` 无论如何都会留下
/// 最近一组，留一条被砍到只剩开头的工具结果，比留一条完整的更没用 —— 模型会照着半句话继续。
const MIN_TOOL_LOOP_HISTORY_CHARS: usize = 2_000;

/// 一条消息在请求里实际占多少字符：`content` 之外还有工具调用的参数。
///
/// 只算 `content` 会漏掉最大的那一类 —— `workspace_write_file` 的整份文件内容在
/// `tool_calls[].arguments` 里，而那条 assistant 消息的 `content` 通常是空串。于是
/// "连写三个大文件"这种最容易顶爆窗口的运行会被估成 0，一次都不修剪，最后照样吃一个
/// "context length exceeded"。
///
/// 图片不算：视觉 token 各家算法不同，硬猜一个只会把估算变成另一个假数字
/// （和 `estimated_prompt_tokens` 同一条理由）。
fn message_cost_chars(message: &ChatMessage) -> usize {
    let mut chars = message.content.chars().count();
    if let Some(calls) = &message.tool_calls {
        for call in calls {
            chars += call.function.name.chars().count() + call.function.arguments.chars().count();
        }
    }
    chars
}

/// 同 `message_cost_chars`，但换成 token 估算
fn message_cost_tokens(message: &ChatMessage) -> usize {
    let mut tokens = estimate_tokens_for_text(&message.content);
    if let Some(calls) = &message.tool_calls {
        for call in calls {
            tokens += estimate_tokens_for_text(&call.function.name)
                + estimate_tokens_for_text(&call.function.arguments);
        }
    }
    tokens
}

/// 把工具回合里长出来的历史裁进这次请求的 token 预算。
///
/// 为什么需要：一次工具回合最多 12 轮，**每一轮都把之前所有消息重发一遍**，而工具调用和
/// 结果是大头（一次 `workspace_read_file` 能带回 64 KB，一次 `workspace_write_file` 能带
/// 出去同样多）。读写几个大文件就能把 128k 的窗口顶满，结果是供应商拒掉整个请求 ——
/// 前面几轮已经花掉的钱和已经落盘的写入，一起变成一句 "context length exceeded"。
/// 裁掉最旧的几组，是这条路上唯一能让运行继续的做法。
///
/// `prompt_len` 之前的消息不动：那是调用方装配好的提示词（系统提示、项目上下文、上游
/// stage 的历史），它们各自有自己的预算，在这里再裁一遍等于两套规则打架。
///
/// 纯函数 + 显式预算，因为这里唯一容易错的是"裁多少"：让它依赖 `LlmClient` 就没法测边界。
fn trim_tool_loop_history(
    messages: &mut Vec<ChatMessage>,
    prompt_len: usize,
    budget_tokens: u32,
) -> Option<HistoryTrim> {
    if messages.len() <= prompt_len {
        return None;
    }
    let tokens_of =
        |slice: &[ChatMessage]| -> usize { slice.iter().map(message_cost_tokens).sum() };
    let fixed_tokens = tokens_of(&messages[..prompt_len]);
    let history_tokens = tokens_of(&messages[prompt_len..]);
    let budget = budget_tokens as usize;
    if fixed_tokens + history_tokens <= budget {
        return None;
    }
    // 提示词自己就超预算时修剪毫无意义：历史砍到只剩最近一组，请求照样装不下，而每一轮
    // 都会再砍一次、再记一条警告。这种情况让供应商明确拒绝 —— 和"窗口未知时不动手"
    // 同一条理由：按一个帮不上忙的判断去丢历史，比一次说得清的失败更难查。
    if fixed_tokens >= budget {
        return None;
    }

    // token → 字符按同一个估算器的实际比例换算。直接把 token 差当字符差会在中文历史上
    // 砍过头（那里 1 字符≈1 token），而按 4:1 硬换又会在 ASCII 上裁不够。
    let allowed_tokens = budget - fixed_tokens;
    let history_chars: usize = messages[prompt_len..].iter().map(message_cost_chars).sum();
    let allowed_chars = history_chars
        .saturating_mul(allowed_tokens)
        .checked_div(history_tokens.max(1))
        .unwrap_or(0)
        .max(MIN_TOOL_LOOP_HISTORY_CHARS);

    let (kept, dropped_exchanges) = bound_transcript_report(
        &messages[prompt_len..],
        MAX_CARRIED_MESSAGE_CHARS,
        allowed_chars,
    );
    let kept_chars: usize = kept.iter().map(message_cost_chars).sum();
    messages.truncate(prompt_len);
    messages.extend(kept);

    // 丢弃量按**组**算（一次工具调用 + 它的结果是一组），因为那才是给用户看的"一次来回"；
    // 按消息条数算还会把声明用的那条系统消息算进去，于是每次都少报一条。
    let removed_chars = history_chars.saturating_sub(kept_chars);
    if dropped_exchanges == 0 && removed_chars == 0 {
        // 一个字都没少：再报一次也没有新信息，而每轮报一次会把 action log 灌满
        return None;
    }
    Some(HistoryTrim {
        dropped_exchanges,
        removed_chars,
        estimated_tokens: (fixed_tokens + history_tokens).min(u32::MAX as usize) as u32,
        budget_tokens,
    })
}

pub async fn execute_stage(
    llm: &LlmClient,
    role: AgentRole,
    stage_name: &str,
    user_prompt: &str,
    context: &str,
    prior_transcript: &[ChatMessage],
    pending_diffs: &str,
    invoker: Option<&dyn ToolInvoker>,
    cancel_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<String>,
) -> Result<StageOutcome, String> {
    let output_rules = match role {
        AgentRole::Architect => "Output a concise implementation plan. Do not output code diffs.",
        AgentRole::Designer => {
            r#"Output one SDD Markdown draft and no source-code diffs. Wrap the document in an `sdd` fence:

```sdd
---
type: sdd
title: Clear design title
version: 1
date: YYYY-MM-DD
status: draft
module: module-or-feature-name
---

# Clear design title

## Problem
...

## Goals
...

## Non-Goals
...

## Proposed Design
...

## User Flows
...

## Interfaces and Data
...

## Acceptance Criteria
...

## Risks
...

## Implementation Notes
...
```

The draft must be specific enough for a later code-mode Agent run to implement it."#
        }
        AgentRole::Coder | AgentRole::Tester => {
            r#"When code changes are needed, prefer the Agent IDE `agent-changes` schema version 1:

```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "path/to/file",
      "baseHash": "optional current file hash when known",
      "rationale": "why this change is needed",
      "hunks": [
        { "original": "exact existing code", "updated": "replacement code" }
      ]
    },
    {
      "type": "create",
      "file": "path/to/new-file",
      "rationale": "why this file is needed",
      "content": "complete file content"
    }
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "path/to/file",
      "hunkIndex": 0,
      "message": "optional reviewer finding tied to a hunk"
    }
  ]
}
```

If you cannot produce valid JSON, use Agent IDE diff/new-file blocks. Use explanations only when no code change is needed.

Your answer has a finite output budget and is cut off without warning when it runs out.
So when you are writing several files, emit **one `agent-changes` block per file** instead
of one block holding them all. Every complete block is kept even if a later one is cut off;
a single block that holds seven files loses all seven. Put the files that matter first."#
        }
        AgentRole::Reviewer => {
            r#"Review the actual pending diffs, not just prior text. Use this structure:

## Review Summary
Short verdict.

## Findings
- [severity] file/path: concrete issue or "No blocking findings".

## Verification
- What should be tested or was implicitly checked.

If a blocking fix is required, include an Agent IDE diff/new-file block after the findings."#
        }
    };

    // 结构：角色系统提示 → 本 stage 的任务消息 → 上游 stage 的真实消息 → 开跑指令。
    // 上游消息里只有 assistant / tool 角色，所以"第一条 user"仍然是本 stage 的任务，
    // 依赖这一点的 mock provider 和本地模型扁平化路径都不受影响。
    let mut messages = vec![
        ChatMessage::system(format!("{}\n\n{}", role.system_prompt(), output_rules)),
        ChatMessage::user(format!(
            "Pipeline stage: {}\nRole: {}\n\nUser task:\n{}\n\nProject context:\n{}\n\nActual pending diffs for review:\n{}",
            stage_name,
            role.to_string(),
            user_prompt,
            context,
            if pending_diffs.trim().is_empty() {
                "No pending diffs."
            } else {
                pending_diffs
            },
        )),
    ];
    messages.extend(bound_transcript(prior_transcript));
    messages.push(ChatMessage::user(if prior_transcript.is_empty() {
        "This is the first stage of the run; there is no prior stage work. Run this stage now."
    } else {
        "Prior stage work is in the messages above, including the actual tool results rather than a retelling of them. Run this stage now."
    }));

    let mut outcome = stream_with_tool_loop(
        llm,
        messages.clone(),
        invoker,
        cancel_flag.clone(),
        tx.clone(),
        MAX_TOOL_ITERATIONS,
    )
    .await?;

    // 输出预算用光时回答会被当场切断，没有任何提示。这里不重试、也不调大上限
    // （被窗口夹住时调大没用，见 `llm_client::window_limited_output_tokens`），
    // 而是把已经写出来的部分留在对话里，另起一条"接着写"的消息续上去：续写的文本
    // 直接拼在后面，第一次没闭合的 JSON 块因此能被补完。
    //
    // 代价要说清楚：每次续写都要把整段上下文连同这半截回答再发一遍，所以既多花钱、
    // 又让剩余的输出空间更小。因此上限很低，而且续不出东西就立刻停。
    let mut continuations = 0;
    while continuations < MAX_OUTPUT_CONTINUATIONS
        && unterminated_fence_diagnostic(&outcome.text, "agent-changes").is_some()
    {
        continuations += 1;
        let mut resumed = messages.clone();
        resumed.push(ChatMessage::assistant(outcome.text.clone()));
        resumed.push(ChatMessage::user(OUTPUT_CONTINUATION_PROMPT.to_string()));
        let next = stream_with_tool_loop(
            llm,
            resumed,
            invoker,
            cancel_flag.clone(),
            tx.clone(),
            MAX_TOOL_ITERATIONS,
        )
        .await?;
        if next.text.trim().is_empty() {
            break;
        }
        outcome.text.push_str(&next.text);
        outcome.transcript.extend(next.transcript);
        outcome.tool_rounds += next.tool_rounds;
        outcome.hit_round_cap |= next.hit_round_cap;
    }
    outcome.output_continuations = continuations;
    Ok(outcome)
}

/// 从 LLM 响应中解析 diff 块
pub fn parse_diffs(response: &str) -> Vec<FileDiff> {
    parse_diffs_with_diagnostics(response).diffs
}

pub fn parse_sdd_artifact(
    response: &str,
    prompt: &str,
    source_run_id: Option<String>,
) -> SddArtifact {
    let raw_markdown = extract_sdd_markdown(response);
    let (mut frontmatter, body) = split_frontmatter(&raw_markdown);
    let title = frontmatter
        .get("title")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| extract_markdown_title(&body))
        .unwrap_or_else(|| summarize_prompt_as_title(prompt));
    let slug = frontmatter
        .get("slug")
        .cloned()
        .filter(|value| is_safe_slug(value))
        .unwrap_or_else(|| slugify(&title));

    frontmatter
        .entry("type".to_string())
        .or_insert_with(|| "sdd".to_string());
    frontmatter
        .entry("title".to_string())
        .or_insert_with(|| title.clone());
    frontmatter
        .entry("version".to_string())
        .or_insert_with(|| "1".to_string());
    frontmatter
        .entry("date".to_string())
        .or_insert_with(|| chrono::Utc::now().date_naive().to_string());
    frontmatter
        .entry("status".to_string())
        .or_insert_with(|| "draft".to_string());
    frontmatter
        .entry("module".to_string())
        .or_insert_with(|| slug.clone());

    let markdown = format!(
        "---\n{}---\n\n{}",
        format_frontmatter(&frontmatter),
        body.trim_start()
    );
    SddArtifact {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        slug,
        frontmatter,
        markdown,
        source_run_id,
        review_findings: Vec::new(),
        status: "draft".to_string(),
    }
}

pub fn extract_review_findings(response: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let mut in_findings = false;
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_findings = trimmed
                .trim_start_matches('#')
                .trim()
                .to_ascii_lowercase()
                .contains("finding");
            continue;
        }
        if in_findings && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            let finding = trimmed[2..].trim();
            if !finding.is_empty() {
                findings.push(finding.to_string());
            }
        }
    }
    findings
}

fn extract_sdd_markdown(response: &str) -> String {
    let lines: Vec<&str> = response.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let rest = trimmed.strip_prefix("```");
        if matches!(rest, Some("sdd") | Some("markdown:sdd") | Some("md:sdd")) {
            let mut block = Vec::new();
            index += 1;
            while index < lines.len() && lines[index].trim() != "```" {
                block.push(lines[index]);
                index += 1;
            }
            return block.join("\n");
        }
        index += 1;
    }
    response.trim().to_string()
}

fn split_frontmatter(markdown: &str) -> (BTreeMap<String, String>, String) {
    let normalized = markdown.trim_start();
    if !normalized.starts_with("---\n") && !normalized.starts_with("---\r\n") {
        return (BTreeMap::new(), normalized.to_string());
    }

    let mut lines = normalized.lines();
    let _ = lines.next();
    let mut frontmatter = BTreeMap::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }
        if in_frontmatter {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    frontmatter.insert(key.to_string(), value.to_string());
                }
            }
        } else {
            body_lines.push(line);
        }
    }
    (frontmatter, body_lines.join("\n"))
}

fn format_frontmatter(frontmatter: &BTreeMap<String, String>) -> String {
    frontmatter
        .iter()
        .map(|(key, value)| format!("{}: {}\n", key, value))
        .collect()
}

fn extract_markdown_title(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let title = line.trim().strip_prefix("# ")?;
        let title = title.trim();
        (!title.is_empty()).then(|| title.to_string())
    })
}

fn summarize_prompt_as_title(prompt: &str) -> String {
    let title = prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Design Specification")
        .chars()
        .take(72)
        .collect::<String>();
    if title.trim().is_empty() {
        "Design Specification".to_string()
    } else {
        title.trim().to_string()
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        format!(
            "design-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    } else {
        slug
    }
}

pub fn is_safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && !value.contains("..")
}

#[derive(Debug, Clone)]
pub struct ParsedDiffs {
    pub diffs: Vec<FileDiff>,
    pub diagnostics: Vec<String>,
}

pub fn parse_diffs_with_diagnostics(response: &str) -> ParsedDiffs {
    let mut diffs = Vec::new();
    let mut diagnostics = Vec::new();
    let lines: Vec<&str> = response.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // 检测代码块开始: ```diff:file, ```new:file, ```lang:file
        if let Some((block_type, file)) = detect_block_start(trimmed) {
            let mut block_lines: Vec<String> = Vec::new();
            i += 1;

            // 收集块内容直到 ```
            while i < lines.len() && lines[i].trim() != "```" {
                block_lines.push(lines[i].to_string());
                i += 1;
            }

            // 没等到收尾的 ``` 就到了回答末尾：这个块是被截断的，不是写完的。
            // 必须整块丢掉。`new`/`code` 块会拿它生成新建文件 diff，Auto 模式直接落盘，
            // 等于把半个文件写上去；`agent-changes` 块只会得到 serde 的
            // "EOF while parsing a string at line 50 column 10856"，说了第几列，
            // 说不出"回答被截断了"——真实运行里用户看到的就是这句加一行 0 new diffs。
            if i >= lines.len() {
                diagnostics.push(cut_off_block_diagnostic(&block_type, &file));
                break;
            }

            match block_type.as_str() {
                "agent-changes" => {
                    let content = block_lines.join("\n");
                    let parsed = parse_agent_changes(&content);
                    diffs.extend(parsed.diffs);
                    diagnostics.extend(parsed.diagnostics);
                }
                "diff" => {
                    let (original, updated) = split_diff_content(&block_lines);
                    let content = block_lines.join("\n");
                    if !content.trim().is_empty() {
                        diffs.push(make_diff(&file, &content, &original, &updated));
                    }
                }
                "new" | "code" => {
                    let content = block_lines.join("\n");
                    if !content.trim().is_empty() {
                        diffs.push(make_new_file_diff(&file, &content));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    ParsedDiffs { diffs, diagnostics }
}

/// 截断诊断里固定出现的这段话。
///
/// 三个地方靠同一段话认出"这次是被截断的"：`was_cut_off`（后端据此把运行判成失败）、
/// `cut_off_block_diagnostic`（生成这句话）、前端 `runFailure.ts`（据此给建议）。
/// 改措辞必须三处一起改，所以这里只有一份常量。
///
/// 特意不用"was cut off"这种短语：`empty_response_error` 里已经有一句
/// "the output was cut off at the output limit"，前端按短语归类会把两类认混，
/// 而那两类的建议不一样（一个是答案空的，一个是改动没生成）。
pub const CUT_OFF_MARKER: &str = "ended before the block closed";

/// 这批诊断里有没有"回答被截断"。
///
/// 截断和别的校验诊断不是一回事：路径不合法只废掉一条改动，被截断则意味着
/// 模型的话没说完，这次的产物本身不完整，不该当成一次跑完的运行。
pub fn was_cut_off(diagnostics: &[String]) -> bool {
    diagnostics.iter().any(|item| item.contains(CUT_OFF_MARKER))
}

/// 整段回答里有没有一个开了没关的围栏块。
///
/// Plan 模式的产物是 markdown（`sdd` 围栏），`detect_block_start` 不认这种块，
/// 所以 `parse_diffs_with_diagnostics` 那条截断判断在 Plan 下永远不成立 ——
/// 被截断的草稿会照样报成功。这里只数围栏行：奇数就说明有一块没收尾。
pub fn unterminated_fence_diagnostic(response: &str, block_type: &str) -> Option<String> {
    let fences = response
        .lines()
        .filter(|line| line.trim().starts_with("```"))
        .count();
    if fences % 2 == 0 {
        return None;
    }
    Some(cut_off_block_diagnostic(block_type, ""))
}

/// 一个没有收尾围栏的代码块该怎么跟用户说。
///
/// 用我们自己的话先说清"回答被截断了、这块没用上"，再让 serde 那类原话跟在后面：
/// 原话说的是第几行第几列，用户没法从中知道该调什么。
///
/// 出路的顺序是有意的：先说"一次少要几个文件"。调大 Max output 常常没用 ——
/// 每次请求的实际上限是 `max_context_tokens - prompt - margin`
/// （`llm_client::window_limited_output_tokens`），已经被窗口夹住时把设置调多大都一样。
fn cut_off_block_diagnostic(block_type: &str, file: &str) -> String {
    let target = if file.trim().is_empty() {
        format!("`{}` block", block_type)
    } else {
        format!("`{}` block for {}", block_type, file)
    };
    format!(
        "The {} was cut off: the response {}, so nothing in it was used. \
         Ask for fewer files in one turn, and one block per file — then a cut-off costs only \
         the last file. Raising Max output does not help when it is already being clamped to \
         fit the context window; this run's action log says whether it was.",
        target, CUT_OFF_MARKER
    )
}

/// 检测代码块类型和文件名: 返回 (类型, 文件名)
fn detect_block_start(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("```")?;
    if rest.is_empty() {
        return None;
    }

    // ```diff:file
    if let Some(file) = rest.strip_prefix("diff:") {
        return Some(("diff".into(), file.trim().to_string()));
    }
    if rest == "diff" {
        return Some(("diff".into(), String::new()));
    }

    if rest == "agent-changes" || rest == "agent_changes" || rest == "json:agent-changes" {
        return Some(("agent-changes".into(), String::new()));
    }

    // ```new:file
    if let Some(file) = rest.strip_prefix("new:") {
        return Some(("new".into(), file.trim().to_string()));
    }

    // ```lang:file (e.g. ```typescript:src/app.ts)
    if let Some(idx) = rest.find(':') {
        let file = rest[idx + 1..].trim();
        if !file.is_empty() && file.contains('.') {
            return Some(("code".into(), file.to_string()));
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct AgentChangesBlock {
    #[serde(default)]
    version: Option<u32>,
    changes: Vec<AgentChange>,
    #[serde(default)]
    findings: Vec<AgentFinding>,
}

#[derive(Debug, Deserialize)]
struct AgentChange {
    #[serde(rename = "type")]
    change_type: String,
    file: String,
    #[serde(rename = "baseHash")]
    base_hash: Option<String>,
    rationale: Option<String>,
    content: Option<String>,
    hunks: Option<Vec<AgentChangeHunk>>,
}

#[derive(Debug, Deserialize)]
struct AgentChangeHunk {
    original: String,
    updated: String,
}

#[derive(Debug, Deserialize)]
struct AgentFinding {
    severity: String,
    file: String,
    #[serde(rename = "hunkIndex")]
    hunk_index: Option<usize>,
    message: String,
}

fn parse_agent_changes(json: &str) -> ParsedDiffs {
    let block = match serde_json::from_str::<AgentChangesBlock>(json) {
        Ok(block) => block,
        Err(err) => {
            return ParsedDiffs {
                diffs: Vec::new(),
                diagnostics: vec![format!("agent-changes JSON parse error: {}", err)],
            };
        }
    };

    let mut diffs = Vec::new();
    let mut diagnostics = Vec::new();
    if block.version != Some(1) {
        diagnostics.push(format!(
            "agent-changes version must be 1; got {:?}",
            block.version
        ));
        return ParsedDiffs { diffs, diagnostics };
    }
    if block.changes.is_empty() {
        diagnostics.push("agent-changes must include at least one change".to_string());
        return ParsedDiffs { diffs, diagnostics };
    }
    let findings = block.findings;
    for (change_index, change) in block.changes.into_iter().enumerate() {
        let change_type = change.change_type.trim();
        let file = change.file.trim();
        if !is_valid_relative_file_path(file) {
            diagnostics.push(format!(
                "agent-changes change {} has invalid relative file path: {}",
                change_index, change.file
            ));
            continue;
        }
        let provenance = DiffProvenance {
            protocol: "agent-changes".to_string(),
            operation: normalized_operation(change_type).to_string(),
            rationale: change
                .rationale
                .clone()
                .filter(|value| !value.trim().is_empty()),
            schema_version: block.version,
            change_index: Some(change_index),
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
            moved_from: None,
        };

        match change_type {
            "create" | "new" => {
                if let Some(content) = change.content {
                    if !content.trim().is_empty() && change.hunks.is_none() {
                        let mut diff = make_new_file_diff(file, &content);
                        diff.provenance = Some(provenance);
                        if let Some(rationale) = change.rationale {
                            if let Some(hunk) = diff.hunks.first_mut() {
                                hunk.content =
                                    format!("rationale: {}\n\n{}", rationale, hunk.content);
                            }
                        }
                        diffs.push(diff);
                    } else {
                        diagnostics.push(format!(
                            "agent-changes create change {} must provide non-empty content and no hunks",
                            change_index
                        ));
                    }
                } else {
                    diagnostics.push(format!(
                        "agent-changes create change {} is missing content",
                        change_index
                    ));
                }
            }
            "edit" | "modify" => {
                if change.content.is_some() {
                    diagnostics.push(format!(
                        "agent-changes edit change {} must use hunks and not content",
                        change_index
                    ));
                    continue;
                };
                let Some(hunks) = change.hunks else {
                    diagnostics.push(format!(
                        "agent-changes edit change {} is missing hunks",
                        change_index
                    ));
                    continue;
                };
                let parsed_hunks: Vec<_> = hunks
                    .into_iter()
                    .enumerate()
                    .filter_map(|(hunk_index, hunk)| {
                        if hunk.original.trim().is_empty() {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} has empty original",
                                change_index, hunk_index
                            ));
                            return None;
                        }
                        if hunk.original == hunk.updated {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} does not change content",
                                change_index, hunk_index
                            ));
                            return None;
                        }
                        if hunk.updated.contains("\u{0000}") || hunk.original.contains("\u{0000}") {
                            diagnostics.push(format!(
                                "agent-changes edit change {} hunk {} contains NUL bytes",
                                change_index, hunk_index
                            ));
                            return None;
                        }
                        let old_count = hunk
                            .original
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                            .max(1) as u32;
                        let new_count = hunk
                            .updated
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                            .max(1) as u32;
                        Some(crate::agent::state_machine::DiffHunk {
                            old_start: 0,
                            old_lines: old_count,
                            new_start: 0,
                            new_lines: new_count,
                            content: change.rationale.clone().unwrap_or_default(),
                            original: hunk.original,
                            updated: hunk.updated,
                            provenance: Some(DiffHunkProvenance {
                                change_index: Some(change_index),
                                hunk_index: Some(hunk_index),
                                source_role: None,
                                source_stage: None,
                                prompt_context: Some(format!(
                                    "agent-changes change {} hunk {}",
                                    change_index, hunk_index
                                )),
                                rationale: change.rationale.clone(),
                            }),
                            status: None,
                        })
                    })
                    .collect();

                if !parsed_hunks.is_empty() {
                    diffs.push(FileDiff {
                        id: uuid::Uuid::new_v4().to_string(),
                        file: file.to_string(),
                        base_hash: change.base_hash,
                        provenance: Some(provenance),
                        hunks: parsed_hunks,
                        status: "pending".to_string(),
                    });
                } else {
                    diagnostics.push(format!(
                        "agent-changes edit change {} has no valid hunks",
                        change_index
                    ));
                }
            }
            _ => diagnostics.push(format!(
                "agent-changes change {} has unsupported type: {}",
                change_index, change.change_type
            )),
        }
    }

    attach_findings_to_hunks(&mut diffs, &findings, &mut diagnostics);

    ParsedDiffs { diffs, diagnostics }
}

fn attach_findings_to_hunks(
    diffs: &mut [FileDiff],
    findings: &[AgentFinding],
    diagnostics: &mut Vec<String>,
) {
    for (finding_index, finding) in findings.iter().enumerate() {
        if finding.message.trim().is_empty() {
            diagnostics.push(format!(
                "agent-changes finding {} has empty message",
                finding_index
            ));
            continue;
        }
        let file = finding.file.trim();
        let Some(diff) = diffs.iter_mut().find(|diff| diff.file == file) else {
            diagnostics.push(format!(
                "agent-changes finding {} references unknown file: {}",
                finding_index, finding.file
            ));
            continue;
        };
        let hunk_index = finding.hunk_index.unwrap_or(0);
        let Some(hunk) = diff.hunks.get_mut(hunk_index) else {
            diagnostics.push(format!(
                "agent-changes finding {} references missing hunk {} in {}",
                finding_index, hunk_index, finding.file
            ));
            continue;
        };
        let provenance = hunk.provenance.get_or_insert_with(|| DiffHunkProvenance {
            change_index: diff
                .provenance
                .as_ref()
                .and_then(|value| value.change_index),
            hunk_index: Some(hunk_index),
            source_role: None,
            source_stage: None,
            prompt_context: None,
            rationale: diff
                .provenance
                .as_ref()
                .and_then(|value| value.rationale.clone()),
        });
        let note = format!(
            "reviewer finding [{}]: {}",
            finding.severity.trim(),
            finding.message.trim()
        );
        provenance.prompt_context = Some(match provenance.prompt_context.as_deref() {
            Some(existing) if !existing.trim().is_empty() => format!("{}\n{}", existing, note),
            _ => note,
        });
    }
}

fn normalized_operation(change_type: &str) -> &'static str {
    match change_type {
        "new" => "create",
        "modify" => "edit",
        "create" => "create",
        "edit" => "edit",
        _ => "unknown",
    }
}

fn is_valid_relative_file_path(file: &str) -> bool {
    if file.is_empty()
        || file.contains('\0')
        || file.starts_with('/')
        || file.starts_with('\\')
        || file.contains("://")
        || std::path::Path::new(file).is_absolute()
    {
        return false;
    }

    let normalized = file.replace('\\', "/");
    !normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
}

/// 分割 diff 内容为 ORIGINAL 和 UPDATED 两部分
fn split_diff_content(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut original = Vec::new();
    let mut updated = Vec::new();
    let mut in_original = false;
    let mut in_updated = false;

    for line in lines {
        let t = line.trim();
        if t.starts_with("<<<<<<<") {
            in_original = true;
            in_updated = false;
            continue;
        }
        if t.starts_with("=======") {
            in_original = false;
            in_updated = true;
            continue;
        }
        if t.starts_with(">>>>>>>") {
            in_original = false;
            in_updated = false;
            continue;
        }
        if in_original {
            original.push(line.clone());
        } else if in_updated {
            updated.push(line.clone());
        }
    }

    (original, updated)
}

fn make_diff(file: &str, content: &str, original: &[String], updated: &[String]) -> FileDiff {
    let old_count = original
        .iter()
        .filter(|l| !l.trim().is_empty())
        .count()
        .max(1) as u32;
    let new_count = updated
        .iter()
        .filter(|l| !l.trim().is_empty())
        .count()
        .max(1) as u32;

    FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        provenance: Some(DiffProvenance {
            protocol: "legacy-diff-block".to_string(),
            operation: "edit".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
            moved_from: None,
        }),
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: old_count,
            new_start: 0,
            new_lines: new_count,
            content: content.to_string(),
            original: original.join("\n"),
            updated: updated.join("\n"),
            provenance: Some(DiffHunkProvenance {
                change_index: None,
                hunk_index: Some(0),
                source_role: None,
                source_stage: None,
                prompt_context: Some("legacy diff block".to_string()),
                rationale: None,
            }),
            status: None,
        }],
        status: "pending".to_string(),
    }
}

fn make_new_file_diff(file: &str, content: &str) -> FileDiff {
    let count = content.lines().count().max(1) as u32;
    FileDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        base_hash: None,
        provenance: Some(DiffProvenance {
            protocol: "legacy-new-block".to_string(),
            operation: "create".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
            moved_from: None,
        }),
        hunks: vec![crate::agent::state_machine::DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: 0,
            new_lines: count,
            content: content.to_string(),
            original: String::new(),
            updated: content.to_string(),
            provenance: Some(DiffHunkProvenance {
                change_index: None,
                hunk_index: Some(0),
                source_role: None,
                source_stage: None,
                prompt_context: Some("legacy new-file block".to_string()),
                rationale: None,
            }),
            status: None,
        }],
        status: "pending".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 带进下一个 stage 的线程是绕过上下文预算直接进请求的，所以它自己必须有上限：
    /// 最近的交换原样保留，更早的整组丢弃并写明丢了多少。
    #[test]
    fn carried_thread_keeps_recent_exchanges_and_states_what_it_dropped() {
        let old = "O".repeat(50);
        let messages = vec![
            ChatMessage::assistant(old.clone()),
            ChatMessage::assistant("second".to_string()),
            ChatMessage::assistant("third".to_string()),
        ];

        let bounded = bound_transcript_with_limits(&messages, 6_000, 12);

        // 最近的两条一个字都不能少：下一个 stage 最依赖紧邻的上一步
        let contents: Vec<&str> = bounded
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert!(contents.contains(&"second"), "{:?}", contents);
        assert!(contents.contains(&"third"), "{:?}", contents);
        assert!(!contents.contains(&old.as_str()), "{:?}", contents);

        // 丢弃量必须写出来：静默丢弃会让模型以为自己看到了完整历史，
        // 然后断言"前面已经验证过了"
        assert_eq!(bounded[0].role, "system");
        assert!(
            bounded[0].content.contains("1 earlier exchange(s)"),
            "{}",
            bounded[0].content
        );
    }

    /// 工具回合里长出来的历史顶到窗口时：裁旧的、留最近的、把丢弃说出来，并且**不动**
    /// 调用方装配的提示词 —— 系统提示和项目上下文各有自己的预算，在这里再裁一遍是两套规则打架。
    #[test]
    fn a_long_tool_loop_trims_its_own_history_and_keeps_the_prompt() {
        let prompt = vec![
            ChatMessage::system("system prompt".to_string()),
            ChatMessage::user("the task".to_string()),
        ];
        let mut messages = prompt.clone();
        // 每组 ≈ 一次工具调用 + 一份 4 000 字符的结果（一次 read_file 就这个量级）
        for index in 0..6 {
            messages.push(ChatMessage::assistant(format!("call {}", index)));
            messages.push(ChatMessage::tool_result(
                format!("call-{}", index),
                format!("result {} ", index).repeat(400),
            ));
        }
        let before = messages.len();

        let trim = trim_tool_loop_history(&mut messages, prompt.len(), 1_000)
            .expect("a 6-round loop of 4 000-char results does not fit 1 000 tokens");

        // 提示词原样留着
        assert_eq!(messages[0].content, "system prompt");
        assert_eq!(messages[1].content, "the task");
        // 最近一轮必须完整留下：留半句话比留一条完整历史更糟
        let last = messages.last().expect("something is kept");
        assert!(last.content.starts_with("result 5"), "{}", last.content);
        assert!(messages.len() < before, "nothing was dropped");
        assert_eq!(trim.budget_tokens, 1_000);
        assert!(trim.estimated_tokens > 1_000, "{:?}", trim);
        // 真正要保证的是"下一次请求变小了"，而不只是"少了几条消息"：按组丢弃之后，
        // 留下的历史加上提示词必须真的进了预算之内
        let cost: usize = messages.iter().map(message_cost_tokens).sum();
        assert!(
            cost <= 1_000,
            "trimmed request is still over budget: {}",
            cost
        );
        // 丢弃量按组算：6 组里留 1 组 ⇒ 丢 5 组。按消息条数算会被那条声明消息带偏
        assert_eq!(trim.dropped_exchanges, 5, "{:?}", trim);
        assert!(trim.removed_chars > 0, "{:?}", trim);
        // 丢弃必须写进历史本身，否则模型会把节选当完整历史
        let note = messages
            .iter()
            .find(|message| message.content.contains("earlier exchange(s) were omitted"))
            .expect("the omission is stated");
        assert_eq!(note.role, "system");
    }

    /// 工具调用的参数也要算进预算。
    ///
    /// `workspace_write_file` 把整份文件放在 `tool_calls[].arguments` 里，那条 assistant
    /// 消息的 `content` 是空串。只算 content 的话，"连写三个大文件"会被估成 0 —— 一次都
    /// 不修剪，最后照样吃一个 "context length exceeded"，而这个函数存在的理由正是它。
    #[test]
    fn a_write_heavy_loop_is_measured_by_its_arguments() {
        let prompt = vec![ChatMessage::system("system prompt".to_string())];
        let mut messages = prompt.clone();
        for index in 0..4 {
            messages.push(ChatMessage::assistant_tool_calls(
                String::new(),
                &[crate::services::llm_client::LlmToolCall {
                    id: format!("call-{}", index),
                    name: "workspace_write_file".to_string(),
                    arguments: format!(
                        "{{\"path\":\"a{}.ts\",\"content\":\"{}\"}}",
                        index,
                        "x".repeat(8_000)
                    ),
                }],
            ));
            messages.push(ChatMessage::tool_result(
                format!("call-{}", index),
                "Wrote 8000 bytes".to_string(),
            ));
        }

        let trim = trim_tool_loop_history(&mut messages, prompt.len(), 1_000)
            .expect("four 8 000-char write calls do not fit 1 000 tokens");

        assert!(trim.dropped_exchanges > 0, "{:?}", trim);
        assert!(trim.estimated_tokens > 1_000, "{:?}", trim);
    }

    /// 提示词自己就超预算时不动历史。
    ///
    /// 砍掉全部历史也装不下，而每一轮都会再砍一次并记一条警告：一个帮不上忙的动作重复
    /// 十二次，只会把 action log 灌满，还让人以为问题在历史上。
    #[test]
    fn a_prompt_that_alone_exceeds_the_budget_is_left_to_the_provider() {
        let prompt = vec![ChatMessage::user("p".repeat(40_000))];
        let mut messages = prompt.clone();
        messages.push(ChatMessage::assistant("call".to_string()));
        messages.push(ChatMessage::tool_result(
            "call-0".to_string(),
            "r".repeat(4_000),
        ));
        let before = messages.len();

        assert!(trim_tool_loop_history(&mut messages, prompt.len(), 1_000).is_none());
        assert_eq!(messages.len(), before, "history was destroyed for nothing");
    }

    /// 装得下就一个字都不要动。
    ///
    /// 这条钉的是"修剪只在超预算时发生"：无条件裁一刀会把短对话里最有用的工具结果也砍掉，
    /// 而那正是模型下一步要引用的东西。
    #[test]
    fn a_short_tool_loop_is_left_alone() {
        let mut messages = vec![
            ChatMessage::system("system prompt".to_string()),
            ChatMessage::user("the task".to_string()),
            ChatMessage::assistant("call".to_string()),
            ChatMessage::tool_result("call-0".to_string(), "short result".to_string()),
        ];
        let before = messages.clone();

        assert!(trim_tool_loop_history(&mut messages, 2, 100_000).is_none());
        assert_eq!(messages.len(), before.len());
        assert_eq!(messages[3].content, before[3].content);
    }

    /// 还没长出历史的那一轮（循环的第一次请求）不该被当成需要修剪。
    #[test]
    fn the_first_round_has_no_history_to_trim() {
        let mut messages = vec![
            ChatMessage::system("a".repeat(40_000)),
            ChatMessage::user("b".repeat(40_000)),
        ];
        // 提示词自己就超预算，但它不属于这里的职责
        assert!(trim_tool_loop_history(&mut messages, 2, 100).is_none());
        assert_eq!(messages.len(), 2);
    }

    /// 单条消息也要有上限，否则一个几十万字符的输出能独自撑爆请求。
    #[test]
    fn carried_thread_truncates_an_oversized_single_message() {
        let huge = "T".repeat(500);
        // 用 assistant 而不是 tool：开头的 tool 消息会被上面那条护栏丢掉，
        // 而这条测试要验证的是"单条超长消息被截断"，角色是无关变量
        let bounded = bound_transcript_with_limits(&[ChatMessage::assistant(huge.clone())], 20, 0);

        assert_eq!(bounded.len(), 1);
        assert!(
            bounded[0].content.contains("earlier character(s) omitted"),
            "{}",
            bounded[0].content
        );
        assert!(bounded[0].content.chars().count() < huge.chars().count());
    }

    /// 供应商要求带 `tool_calls` 的 assistant 消息后面必须紧跟对应的 `tool` 结果。
    /// 只丢一半不是"少点历史"，而是整个请求 400、这一 stage 直接失败 ——
    /// 所以裁剪只能按整组进行。
    #[test]
    fn carried_thread_never_splits_a_tool_call_from_its_result() {
        let call = LlmToolCall {
            id: "call-1".to_string(),
            name: "workspace_read_file".to_string(),
            arguments: "{}".to_string(),
        };
        let messages = vec![
            ChatMessage::assistant("earlier chatter".to_string()),
            ChatMessage::assistant_tool_calls("reading the file".to_string(), &[call]),
            ChatMessage::tool_result("call-1", "file contents".to_string()),
        ];

        // 预算只够最后一组
        let bounded = bound_transcript_with_limits(&messages, 6_000, 30);

        let roles: Vec<&str> = bounded
            .iter()
            .map(|message| message.role.as_str())
            .collect();
        assert_eq!(roles, vec!["system", "assistant", "tool"], "{:?}", roles);
        assert_eq!(bounded[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn carried_thread_is_empty_when_no_stage_has_run() {
        assert!(bound_transcript(&[]).is_empty());
    }

    /// 附图那句话必须说清"是哪些"，不然尾部截断这个设计对模型就是不可见的。
    #[test]
    fn the_image_note_says_which_images_are_missing_and_not_to_reread_them() {
        assert_eq!(
            round_image_note(2, 2),
            "Attached 2 image(s) from the tool call(s) above."
        );

        let note = round_image_note(1, 3);
        assert!(note.contains("first 1 of 3"), "{}", note);
        // 少了几张、少的是哪几张，两件事都要说
        assert!(note.contains("The last 2"), "{}", note);
        // 重读会撞运行预算，所以必须明确劝住
        assert!(note.contains("do not read them again"), "{}", note);
        assert!(note.contains("next step"), "{}", note);
    }

    /// orchestrator 把 `[Stage / role]` 标签加在消息**头部**，而尾部截断会先吃掉头部。
    /// 于是超长阶段输出恰好丢掉归属 —— 长输出正是最需要知道"这是谁说的"的时候。
    #[test]
    fn carried_thread_keeps_the_provenance_label_of_a_long_message() {
        let long = format!("[Architect / architect]\n{}", "detail\n".repeat(400));
        let long_len = long.chars().count();

        let bounded = bound_transcript_with_limits(
            &[ChatMessage::assistant(long)],
            120,
            crate::agent::executor::MAX_CARRIED_THREAD_CHARS,
        );

        assert_eq!(bounded.len(), 1);
        assert!(
            bounded[0].content.starts_with("[Architect / architect]\n"),
            "{}",
            bounded[0].content
        );
        // 仍然要保尾部并写明省略量：结论和 diff 都在末尾
        assert!(
            bounded[0].content.contains("earlier character(s) omitted"),
            "{}",
            bounded[0].content
        );
        assert!(
            bounded[0].content.ends_with("detail\n"),
            "{}",
            bounded[0].content
        );
        // 不断言精确长度：`truncate_for_prompt` 会在预算之外再加一行省略标记，
        // 写死一个数字只会变成测实现细节。够短就行。
        assert!(
            bounded[0].content.chars().count() < long_len / 4,
            "{}",
            bounded[0].content
        );
    }

    /// 供应商要求每条 `tool` 消息前面必须有发起它的 assistant 调用。分组逻辑依赖
    /// "线程不会以 tool 消息开头"这个前提；这条测试把前提本身钉住，避免以后某个
    /// 新的续跑入口传进一段以 tool 开头的历史，导致整个请求被拒。
    #[test]
    fn carried_thread_never_emits_a_tool_message_without_its_assistant_call() {
        let orphan = vec![
            ChatMessage::tool_result("call-1", "orphaned result".to_string()),
            ChatMessage::assistant("later answer".to_string()),
        ];

        let bounded = bound_transcript(&orphan);

        let first_tool = bounded.iter().position(|message| message.role == "tool");
        if let Some(index) = first_tool {
            assert!(index > 0, "tool 消息不能是第一条: {:?}", bounded);
            assert!(
                bounded[index - 1].tool_calls.is_some(),
                "tool 消息前面必须是发起它的 assistant 调用: {:?}",
                bounded
            );
        }
    }

    /// 跑一个 mock 端点上的工具循环：第一轮模型会调一次 `stub_probe`，之后直接作答。
    ///
    /// 用它把"轮数/触顶"这两个数字钉死在循环的记账上，而不是事后从 transcript 反推。
    fn loop_with_mock_tool(max_iterations: usize) -> StageOutcome {
        let invoker = RecordingInvoker::new("stub_");
        let _guard = crate::services::workspace::env_test_guard();
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "stub_probe");
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://rounds".to_string(),
                api_key: "sk-test".to_string(),
                model: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                max_context_tokens: None,
                reasoning_effort: None,
                max_output_tokens: None,
                tool_call_mode: "native".to_string(),
                model_type: crate::services::llm_client::ModelType::OpenAI,
                local_model_config: None,
            })
            .with_extra_tools(vec![crate::services::llm_client::ToolDefinition {
                name: "stub_probe".to_string(),
                description: "probe stub".to_string(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            }]);
        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(64);
        let outcome = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(stream_with_tool_loop(
                &llm,
                vec![ChatMessage::user("probe it")],
                Some(&invoker),
                std::sync::Arc::new(AtomicBool::new(false)),
                tx,
                max_iterations,
            ));
        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        outcome.expect("mock loop")
    }

    /// 用掉了每一轮、然后正常作答的循环**不是**被截断的循环。
    ///
    /// 这是旧写法（`rounds >= 上限`）报错的那一半：最后一轮本来就不执行工具，所以"用满 N 轮
    /// 之后给出答案"和"第 N 轮被拦下"在 transcript 上分不开 —— 而子 Agent 会把前者当成
    /// "这个答案可能不完整"转述给主 Agent。
    #[test]
    fn a_loop_that_answered_after_using_every_round_is_not_reported_as_truncated() {
        let outcome = loop_with_mock_tool(1);
        assert_eq!(outcome.tool_rounds, 1);
        assert!(!outcome.hit_round_cap, "{}", outcome.text);
        assert!(
            !outcome.text.contains("Tool loop stopped"),
            "{}",
            outcome.text
        );
    }

    /// 上限那一轮还有没执行的工具调用，就必须说出来 —— 对调用方那是一段没跑完的探索。
    #[test]
    fn a_loop_that_still_wanted_tools_at_the_cap_says_so() {
        let outcome = loop_with_mock_tool(0);
        assert_eq!(outcome.tool_rounds, 0);
        assert!(outcome.hit_round_cap);
        assert!(
            outcome.text.contains("Tool loop stopped"),
            "{}",
            outcome.text
        );
    }

    struct RecordingInvoker {
        prefix: &'static str,
        calls: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl RecordingInvoker {
        fn new(prefix: &'static str) -> Self {
            Self {
                prefix,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolInvoker for RecordingInvoker {
        fn handles(&self, tool_name: &str) -> bool {
            tool_name.starts_with(self.prefix)
        }

        async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push((tool_name.to_string(), arguments.to_string()));
            Ok("ok".to_string())
        }
    }

    fn call(name: &str) -> LlmToolCall {
        LlmToolCall {
            id: format!("call_{}", name),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    /// 一个请求装不下的图片要**留到下一轮**，而不是丢掉，而且每张只发一次。
    ///
    /// ROADMAP 76 把这条记为"只有纯函数覆盖"。端到端能跑通的关键是 mock provider 判
    /// "这一轮调过工具没有"只看最后一条 `user` 消息之后 —— 附图本身就是一条新的 `user`
    /// 消息，所以下一轮它会再发一次工具调用，真实的多轮循环就出现了。
    #[test]
    fn an_image_that_does_not_fit_one_request_rides_the_next_one() {
        struct ImageInvoker {
            handed_out: std::sync::Mutex<bool>,
            images: std::sync::Mutex<Vec<crate::services::images::ImagePart>>,
        }

        #[async_trait]
        impl ToolInvoker for ImageInvoker {
            fn handles(&self, tool_name: &str) -> bool {
                tool_name == "stub_capture"
            }

            async fn invoke(&self, _tool_name: &str, _arguments: &str) -> Result<String, String> {
                Ok("captured".to_string())
            }

            fn take_images(&self) -> Vec<crate::services::images::ImagePart> {
                let mut handed_out = self.handed_out.lock().unwrap();
                if *handed_out {
                    return Vec::new();
                }
                *handed_out = true;
                std::mem::take(&mut self.images.lock().unwrap())
            }
        }

        // 两张各占一多半的图：第一张留下，第二张必须推到下一轮
        let half = crate::services::images::MAX_REQUEST_IMAGE_BASE64_BYTES / 2 + 1;
        let image = |marker: char| crate::services::images::ImagePart {
            media_type: "image/png".to_string(),
            base64_data: std::iter::repeat_n(marker, half).collect::<String>(),
        };
        let invoker = ImageInvoker {
            handed_out: std::sync::Mutex::new(false),
            images: std::sync::Mutex::new(vec![image('A'), image('B')]),
        };

        let _guard = crate::services::workspace::env_test_guard();
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "stub_capture");
        let recorder = std::sync::Arc::new(crate::services::llm_client::RequestRecorder::new());
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://images".to_string(),
                api_key: "sk-test".to_string(),
                model: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                max_context_tokens: None,
                reasoning_effort: None,
                max_output_tokens: None,
                tool_call_mode: "native".to_string(),
                model_type: crate::services::llm_client::ModelType::OpenAI,
                local_model_config: None,
            })
            .with_extra_tools(vec![crate::services::llm_client::ToolDefinition {
                name: "stub_capture".to_string(),
                description: "capture stub".to_string(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            }])
            .with_request_recorder(recorder.clone());

        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(64);
        let outcome = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(stream_with_tool_loop(
                &llm,
                vec![ChatMessage::user("look at both")],
                Some(&invoker),
                std::sync::Arc::new(AtomicBool::new(false)),
                tx,
                MAX_TOOL_ITERATIONS,
            ));
        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        assert!(outcome.is_ok(), "{:?}", outcome);

        let requests = recorder.requests();
        let images_per_request: Vec<usize> = requests
            .iter()
            .map(|messages| messages.iter().map(|message| message.images.len()).sum())
            .collect();
        // 第一次请求还没有图；之后两次各带一张；最后一次已经清空 —— 每张只发一次
        assert_eq!(images_per_request, vec![0, 1, 1, 0], "{:?}", requests);

        let marker_of = |index: usize| {
            requests[index]
                .iter()
                .flat_map(|message| message.images.iter())
                .map(|image| image.base64_data.chars().next().unwrap_or('?'))
                .collect::<Vec<char>>()
        };
        assert_eq!(marker_of(1), vec!['A']);
        // 顺序不能乱：推迟的是尾部那张，下一轮补的还是它
        assert_eq!(marker_of(2), vec!['B']);

        let note = requests[1]
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
            .unwrap_or_default();
        assert!(note.contains("first 1 of 2"), "{}", note);
        assert!(note.contains("do not read them again"), "{}", note);

        // 两张图各自进了一次请求，所以 mock 端点（拍平成文本、看不了图）会各报一次降级。
        // 这条断言比"没有降级"更强：它证明推迟的那张**真的**搭上了后一次请求 —— 如果
        // 它被丢掉了，这里只会有一条记录。
        let drops = llm.image_drops();
        assert_eq!(drops.len(), 2, "{:?}", drops);
        assert!(
            drops
                .iter()
                .all(|drop| drop.count == 1 && drop.reason.contains("flattened text prompt")),
            "{:?}",
            drops
        );
    }

    #[test]
    fn external_calls_exclude_builtin_output_protocol_tools() {
        let invoker = RecordingInvoker::new("mcp__");
        let calls = vec![
            call("emit_agent_changes"),
            call("mcp__files__read"),
            call("emit_sdd_draft"),
            call("mcp__git__log"),
        ];

        let selected = select_external_calls(&calls, Some(&invoker));

        assert_eq!(
            selected
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
            vec!["mcp__files__read", "mcp__git__log"]
        );
    }

    #[test]
    fn external_calls_are_empty_without_invoker() {
        assert!(select_external_calls(&[call("mcp__files__read")], None).is_empty());
    }

    #[test]
    fn merge_tool_call_output_appends_agent_changes_block() {
        let merged = merge_tool_call_output(LlmStreamOutput {
            content: "Applying the rename.".to_string(),
            tool_calls: vec![crate::services::llm_client::LlmToolCall {
                id: "call_1".to_string(),
                name: "emit_agent_changes".to_string(),
                arguments: r#"{"version":1,"changes":[{"type":"edit","file":"src/app.ts","hunks":[{"original":"const a = 1;","updated":"const a = 2;"}]}]}"#.to_string(),
            }],
            usage: None,
        });

        assert!(merged.starts_with("Applying the rename."));
        assert!(merged.contains("```agent-changes"));

        // 合成块必须能被现有 parse_diffs 管线解析（传输方式对下游透明）
        let diffs = parse_diffs(&merged);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file, "src/app.ts");
    }

    #[test]
    fn merge_tool_call_output_keeps_plain_content_when_no_usable_calls() {
        let base = LlmStreamOutput {
            content: "Just prose.".to_string(),
            tool_calls: vec![crate::services::llm_client::LlmToolCall {
                id: "call_2".to_string(),
                name: "emit_agent_changes".to_string(),
                arguments: r#"{"version":1,"changes":[]}"#.to_string(),
            }],
            usage: None,
        };
        assert_eq!(merge_tool_call_output(base), "Just prose.");

        let plain = LlmStreamOutput {
            content: "Just prose.".to_string(),
            tool_calls: Vec::new(),
            usage: None,
        };
        assert_eq!(merge_tool_call_output(plain), "Just prose.");
    }

    #[test]
    fn parse_diffs_supports_structured_agent_changes() {
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "src/app.ts",
      "rationale": "rename value",
      "hunks": [
        {
          "original": "const value = 1;",
          "updated": "const value = 2;"
        }
      ]
    },
    {
      "type": "create",
      "file": "src/new.ts",
      "rationale": "add helper",
      "content": "export const helper = true;\n"
    }
  ]
}
```"#;

        let diffs = parse_diffs(response);

        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].file, "src/app.ts");
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().protocol,
            "agent-changes"
        );
        assert_eq!(diffs[0].provenance.as_ref().unwrap().operation, "edit");
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().schema_version,
            Some(1)
        );
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().rationale.as_deref(),
            Some("rename value")
        );
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
        assert_eq!(
            diffs[0].hunks[0].provenance.as_ref().unwrap().change_index,
            Some(0)
        );
        assert_eq!(diffs[1].file, "src/new.ts");
        assert_eq!(diffs[1].provenance.as_ref().unwrap().operation, "create");
        assert_eq!(diffs[1].hunks[0].updated, "export const helper = true;\n");
    }

    #[test]
    fn parse_diffs_keeps_legacy_diff_block_support() {
        let response = r#"```diff:src/app.ts
<<<<<<< ORIGINAL
const value = 1;
=======
const value = 2;
>>>>>>> UPDATED
```"#;

        let diffs = parse_diffs(response);

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file, "src/app.ts");
        assert_eq!(
            diffs[0].provenance.as_ref().unwrap().protocol,
            "legacy-diff-block"
        );
        assert_eq!(diffs[0].hunks[0].original, "const value = 1;");
        assert_eq!(diffs[0].hunks[0].updated, "const value = 2;");
    }

    #[test]
    fn parse_diffs_rejects_invalid_structured_agent_changes() {
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "../outside.ts",
      "hunks": [
        { "original": "const value = 1;", "updated": "const value = 2;" }
      ]
    },
    {
      "type": "edit",
      "file": "src/same.ts",
      "hunks": [
        { "original": "const value = 1;", "updated": "const value = 1;" }
      ]
    },
    {
      "type": "create",
      "file": "src/mixed.ts",
      "content": "export {};",
      "hunks": [
        { "original": "old", "updated": "new" }
      ]
    }
  ]
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn parse_diffs_reports_structured_validation_errors() {
        let response = r#"```agent-changes
{
  "version": 2,
  "changes": []
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(parsed
            .diagnostics
            .iter()
            .any(|item| item.contains("version must be 1")));
    }

    #[test]
    fn a_cut_off_agent_changes_block_says_it_was_cut_off() {
        // 真实运行的形状：输出上限把 JSON 截在字符串中间，收尾的 ``` 永远没来。
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "create",
      "file": "dynproxy/protocol.py",
      "content": "'''Wire format for the tunnel.\n\nMAGIC = b'DPX1'"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(parsed
            .diagnostics
            .iter()
            .any(|item| item.contains("was cut off")));
    }

    #[test]
    fn the_cut_off_wording_is_pinned_for_the_frontend() {
        // 前端 `runFailure.ts` 按这句话的字面量归类。用 CUT_OFF_MARKER 断言等于让常量
        // 自证（恒真）：改词之后两边测试全绿，线上却会因为消息里含 "fit the context
        // window" 被归到 contextLimit，给出"少发上下文"这句相反的建议。所以这里写死字面量。
        let diagnostic = cut_off_block_diagnostic("agent-changes", "");
        assert!(diagnostic.contains("ended before the block closed"));
        assert_eq!(CUT_OFF_MARKER, "ended before the block closed");
    }

    #[test]
    fn complete_blocks_survive_a_cut_off_later_block() {
        // 这是"一个文件一个块"这条提示存在的理由：前面写完的块照样落地，
        // 被截断的只赔掉最后一个文件。一个块装七个文件则七个全丢。
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    { "type": "create", "file": "dynproxy/protocol.py", "content": "MAGIC = b'DPX1'\n" }
  ]
}
```

```agent-changes
{
  "version": 1,
  "changes": [
    { "type": "create", "file": "dynproxy/server.py", "content": "import asyncio"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert_eq!(parsed.diffs.len(), 1);
        assert_eq!(parsed.diffs[0].file, "dynproxy/protocol.py");
        assert!(parsed
            .diagnostics
            .iter()
            .any(|item| item.contains(CUT_OFF_MARKER)));
    }

    #[test]
    fn a_cut_off_new_file_block_writes_nothing() {
        // 这一类最危险：截断的块以前会变成一个新建文件 diff，Auto 模式直接落盘半个文件。
        let response = "```new:src/half.ts\nexport function half() {\n  return 1";

        let parsed = parse_diffs_with_diagnostics(response);

        assert!(parsed.diffs.is_empty());
        assert!(parsed
            .diagnostics
            .iter()
            .any(|item| item.contains("src/half.ts") && item.contains("was cut off")));
    }

    #[test]
    fn parse_diffs_attaches_review_findings_to_hunk_provenance() {
        let response = r#"```agent-changes
{
  "version": 1,
  "changes": [
    {
      "type": "edit",
      "file": "src/app.ts",
      "rationale": "fix value",
      "hunks": [
        {
          "original": "const value = 1;",
          "updated": "const value = 2;"
        }
      ]
    }
  ],
  "findings": [
    {
      "severity": "warning",
      "file": "src/app.ts",
      "hunkIndex": 0,
      "message": "verify value usage"
    }
  ]
}
```"#;

        let parsed = parse_diffs_with_diagnostics(response);

        assert_eq!(parsed.diffs.len(), 1);
        let hunk_provenance = parsed.diffs[0].hunks[0]
            .provenance
            .as_ref()
            .expect("hunk provenance");
        assert!(hunk_provenance
            .prompt_context
            .as_deref()
            .unwrap_or_default()
            .contains("verify value usage"));
    }

    #[test]
    fn parse_sdd_artifact_normalizes_frontmatter_and_slug() {
        let artifact = parse_sdd_artifact(
            r#"```sdd
---
type: sdd
title: Token Budget Meter
version: 1
status: draft
module: chat
---

# Token Budget Meter

## Goals
- Show budget usage.
```"#,
            "Build token budget UI",
            Some("run-1".to_string()),
        );

        assert_eq!(artifact.title, "Token Budget Meter");
        assert_eq!(artifact.slug, "token-budget-meter");
        assert_eq!(artifact.source_run_id.as_deref(), Some("run-1"));
        assert_eq!(
            artifact.frontmatter.get("type").map(String::as_str),
            Some("sdd")
        );
        assert!(artifact.markdown.starts_with("---\n"));
    }

    #[test]
    fn parse_sdd_artifact_adds_required_frontmatter_when_missing() {
        let artifact = parse_sdd_artifact(
            "# Python LSP\n\n## Goals\n- Diagnostics",
            "Python LSP",
            None,
        );

        assert_eq!(artifact.title, "Python LSP");
        assert_eq!(artifact.slug, "python-lsp");
        assert_eq!(
            artifact.frontmatter.get("status").map(String::as_str),
            Some("draft")
        );
        assert!(artifact.markdown.contains("type: sdd"));
    }

    #[test]
    fn sdd_slug_validation_rejects_path_traversal() {
        assert!(is_safe_slug("token-budget-meter"));
        assert!(!is_safe_slug("../secret"));
        assert!(!is_safe_slug("feature/name"));
    }
}
