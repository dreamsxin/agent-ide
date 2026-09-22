use std::path::PathBuf;

pub const PROJECT_MEMORY_FILE: &str = "AGENTS.md";
pub const MAX_PROJECT_MEMORY_CHARS: usize = 8_000;

/// 项目记忆此刻的状态，外加那一个能改变它的动作。
///
/// 两件事放在一起，因为它们只在一起才有用：界面要么说"这个项目没有 AGENTS.md，Agent 只能
/// 按通用习惯干活"，要么说"它有 9.2 KB，最后 1.2 KB 根本没发给模型"。两种情况下用户想做的
/// 都是同一件事，而那句提示词就在手边。
///
/// 截断这件事以前只有模型知道（注入的文本末尾有一句 `project memory truncated`），用户看不到
/// 任何症状 —— 他写的规则从某一行之后就静默失效了。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemoryInfo {
    /// 有没有真的打开了工作区。
    ///
    /// `workspace_root()` 在什么都没打开时会退回到进程的当前目录 —— 开发环境里于是读到
    /// **本仓库**的 `AGENTS.md`，界面照着它说"这个工作区有一份 7 KB 的项目记忆"，而用户
    /// 什么都没打开。这个字段存在就是为了不说那句话。
    pub workspace_open: bool,
    pub exists: bool,
    /// 这份文件该在哪；不存在时也给出来，用户要知道该在哪新建
    pub path: String,
    /// 磁盘上的字节数（trim 之后，和注入时算的是同一个数）
    pub bytes: usize,
    /// 注入前的硬上限。装得下不等于全部都会进提示词：上下文预算还会按配额再削一次
    /// （`context.rs` 里项目记忆那一节占 15%），聊天里也能把这一节整个关掉。
    pub limit: usize,
    /// 超了上限：尾部不会进入任何一次运行
    pub truncated: bool,
    /// 让 Agent 起草/更新这份文件的提示词。前端按普通提问发出去 —— 它写文件走的是
    /// 平常那条审查 + 撤销的路，没有任何新权限。
    pub draft_prompt: String,
}

pub fn project_memory_path() -> Result<PathBuf, String> {
    let root = crate::services::workspace::workspace_root()?;
    Ok(root.join(PROJECT_MEMORY_FILE))
}

/// 读一次项目记忆的状态。文件不存在不是错误 —— 那是最常见的情况。
pub fn project_memory_info() -> Result<ProjectMemoryInfo, String> {
    let path = project_memory_path()?;
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("Read {}: {}", path.display(), err)),
    };
    let bytes = content
        .as_deref()
        .map(|text| text.trim().len())
        .unwrap_or(0);
    Ok(ProjectMemoryInfo {
        workspace_open: crate::services::workspace::current_workspace_key().is_some(),
        exists: content.is_some(),
        path: path.display().to_string(),
        bytes,
        limit: MAX_PROJECT_MEMORY_CHARS,
        truncated: bytes > MAX_PROJECT_MEMORY_CHARS,
        draft_prompt: draft_project_memory_prompt(content.is_some(), bytes),
    })
}

/// 让 Agent 起草（或更新）项目记忆的提示词。
///
/// 纯函数，因为这段文字就是这个功能的全部：没有专门的代码路径，Agent 用它平常的读写工具
/// 干活，产出的改动照样经过审查区和撤销栈。这也意味着提示词里的每一条约束都是"说给模型听的"
/// 而不是强制的 —— 所以要写清**为什么**，让它有理由照做。
///
/// 三条约束是这份文件真正的成败所在：
/// - **只写查得到的事实。** 一份编出来的构建命令比没有文档更糟：它会被后面每一次运行当成
///   真相，而第一次照着跑的人才会发现它是假的。
/// - **有上限。** 它被注入每一次运行，超过 8 000 字节的部分静默消失 —— 尾部恰好是最后写的
///   那几条规则。
/// - **已经有就改，不要整份重写。** 那份文件里往往有人手写的、代码里查不到的约定（一次事故
///   留下的规矩），整份重写会把它们抹掉，而抹掉这件事在 diff 里一眼看不出来。
pub fn draft_project_memory_prompt(exists: bool, bytes: usize) -> String {
    let opening = if exists {
        format!(
            "This workspace already has {} ({} bytes). Read it first, then update it in place — \
             keep every rule that is still true, and do not rewrite the file wholesale: it may \
             contain conventions that exist nowhere in the code (a rule left behind by an \
             incident), and losing those is not visible in a diff.",
            PROJECT_MEMORY_FILE, bytes
        )
    } else {
        format!(
            "This workspace has no {} yet, so every Agent run here starts without any project \
             conventions. Create it at the workspace root.",
            PROJECT_MEMORY_FILE
        )
    };
    format!(
        "Draft this project's {file}\n\n\
         {opening}\n\n\
         Write the file for the next Agent that works here, not for a human reader. Before you \
         write anything, find the evidence: read the package manifests and lockfiles, the scripts \
         and task definitions, the CI config, the test layout, and the top-level directories. \
         Prefer what a config file says over what a README claims.\n\n\
         Cover, in this order, only what this repo actually has:\n\
         1. What the project is and which directories matter.\n\
         2. The exact commands to build, type-check, lint and run tests — copied from where they \
         are defined, not guessed. Say which one is the real suite if there are several.\n\
         3. Architecture rules a newcomer would violate: layer boundaries, what must not import \
         what, where a particular kind of code belongs.\n\
         4. Conventions that are not obvious from one file: naming, error handling, logging, how \
         tests are written.\n\
         5. Traps that have already cost someone time — but only if you can point at the code or \
         config that proves them.\n\n\
         Hard constraints:\n\
         - **Only facts you verified in this repo.** An invented build command is worse than no \
         file at all: it is treated as truth by every later run, and the person who follows it is \
         the one who finds out.\n\
         - **No secrets, tokens, or absolute paths from this machine.**\n\
         - **Stay under {limit} bytes** (aim for {target}). The backend injects this file into \
         every Agent run and silently drops everything past that limit — the tail, which is \
         exactly where the last rules you wrote would be.\n\
         - Say nothing you would have to invent to say. A short file that is entirely true is the \
         goal.\n\n\
         Propose the file as a normal change so it lands in the review area; then summarise in two \
         or three lines what you put in it and what you deliberately left out because you could not \
         verify it.",
        opening = opening,
        file = PROJECT_MEMORY_FILE,
        limit = MAX_PROJECT_MEMORY_CHARS,
        target = MAX_PROJECT_MEMORY_CHARS - 500,
    )
}

/// Load the workspace-root `AGENTS.md` project memory file.
///
/// Returns `Ok(None)` when the file does not exist. Content is trimmed and
/// bounded so it stays predictable inside context budget packing.
///
/// 返回的是文本**加上"被截断了没有"**：只回一个 `String` 的时候，这件事只能靠在注入文本里
/// 搜一句标记才能知道，于是它实际上只有模型看得见 —— 而用户看到的是"规则写了但 Agent 不照做"。
pub fn load_project_memory() -> Result<Option<LoadedProjectMemory>, String> {
    let path = project_memory_path()?;
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Read {}: {}", path.display(), err)),
    };
    let bytes = content.trim().len();
    Ok(Some(LoadedProjectMemory {
        text: bound_project_memory(&content),
        bytes,
        truncated: bytes > MAX_PROJECT_MEMORY_CHARS,
    }))
}

/// 读出来的项目记忆：进提示词的那段文本，外加"原文多大、被切了没有"。
pub struct LoadedProjectMemory {
    pub text: String,
    /// 磁盘上 trim 之后的字节数
    pub bytes: usize,
    pub truncated: bool,
}

/// 把"项目记忆被截断"变成一句给用户的话。
///
/// 措辞要说清丢的是**尾部**：那正是用户最后写下的几条规则，也是他最可能以为还生效的那几条。
/// 说"至多前 N 字节"而不是"前 N 字节"：切口要退回字符边界（最多少 3 字节），而上下文装配
/// 之后还会按配额把这一节再削一次、甚至整节丢掉 —— 给一个精确数字等于给一个查不实的承诺。
/// 单独一个纯函数是为了能测，理由同 `history_trim_report`。
pub fn truncation_report(bytes: usize) -> (String, String) {
    (
        format!(
            "{} is {} bytes; at most the first {} reached the model",
            PROJECT_MEMORY_FILE, bytes, MAX_PROJECT_MEMORY_CHARS
        ),
        format!(
            "This file is injected into every run, and everything past {} bytes is dropped before \
             the request is even assembled — the tail, which is where the most recently added rules \
             are. The Agent did not see them in this run, and a tight context budget can trim what \
             is left even further. Shorten {} (Settings shows its size, and the Agent can tighten it \
             for you).",
            MAX_PROJECT_MEMORY_CHARS, PROJECT_MEMORY_FILE
        ),
    )
}

pub fn bound_project_memory(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.len() <= MAX_PROJECT_MEMORY_CHARS {
        return trimmed.to_string();
    }
    format!(
        "{}\n\n/* ... project memory truncated ... */",
        safe_prefix(trimmed, MAX_PROJECT_MEMORY_CHARS)
    )
}

fn safe_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn bound_project_memory_keeps_small_content() {
        let bounded = bound_project_memory("# Agent rules\n\nAlways run cargo test.\n");

        assert_eq!(bounded, "# Agent rules\n\nAlways run cargo test.");
    }

    #[test]
    fn bound_project_memory_truncates_oversized_content() {
        let content = format!("# Rules\n\n{}", "x".repeat(MAX_PROJECT_MEMORY_CHARS + 100));

        let bounded = bound_project_memory(&content);

        assert!(bounded.contains("project memory truncated"));
        assert!(bounded.len() <= MAX_PROJECT_MEMORY_CHARS + 80);
    }

    #[test]
    fn bound_project_memory_respects_char_boundaries() {
        let content = "中文".repeat((MAX_PROJECT_MEMORY_CHARS / 6) + 10);

        let bounded = bound_project_memory(&content);

        assert!(bounded.contains("project memory truncated"));
    }

    #[test]
    fn load_project_memory_reads_workspace_agents_md() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp =
            std::env::temp_dir().join(format!("agent-ide-project-memory-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();
        std::fs::write(
            temp.join(PROJECT_MEMORY_FILE),
            "# Rules\n\nAlways run cargo test.\n",
        )
        .unwrap();

        let memory = load_project_memory().unwrap().expect("project memory");

        assert!(memory.text.contains("Always run cargo test."));
        assert!(!memory.truncated);
        assert_eq!(memory.bytes, "# Rules\n\nAlways run cargo test.".len());

        let _ = std::fs::remove_dir_all(temp);
    }

    /// 截断这件事必须能被调用方看见，而不只是藏在注入文本里的一句标记。
    ///
    /// 只回一个 `String` 的时候，唯一能还原它的办法是搜那句标记 —— 于是实际上只有模型看得见，
    /// 用户看到的是"我写的规则 Agent 不照做"。
    #[test]
    fn load_project_memory_says_when_it_had_to_cut_the_tail() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!("agent-ide-memory-cut-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();
        std::fs::write(
            temp.join(PROJECT_MEMORY_FILE),
            "x".repeat(MAX_PROJECT_MEMORY_CHARS + 300),
        )
        .unwrap();

        let memory = load_project_memory().unwrap().expect("project memory");

        assert!(memory.truncated);
        assert_eq!(memory.bytes, MAX_PROJECT_MEMORY_CHARS + 300);
        assert!(memory.text.contains("project memory truncated"));

        let _ = std::fs::remove_dir_all(temp);
    }

    /// 那句话要说清丢的是**尾部**，也要给出原文多大 —— 用户只有这两个数字能据以行动。
    #[test]
    fn the_truncation_report_names_the_tail_and_the_size() {
        let (summary, details) = truncation_report(9_200);

        assert!(summary.contains("9200"), "{}", summary);
        assert!(
            summary.contains(&MAX_PROJECT_MEMORY_CHARS.to_string()),
            "{}",
            summary
        );
        assert!(details.contains("tail"), "{}", details);
        // 不能只说"被截断了"：要说这一次运行里模型没看到它们
        assert!(
            details.contains("did not see them in this run"),
            "{}",
            details
        );
    }

    #[test]
    fn load_project_memory_returns_none_when_missing() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!(
            "agent-ide-project-memory-missing-test-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();

        assert!(load_project_memory().unwrap().is_none());

        let _ = std::fs::remove_dir_all(temp);
    }

    /// 状态要说清是哪一种"没在用"。
    ///
    /// "这个项目没有 AGENTS.md"和"它有但尾部被切掉了"在效果上都是"规则没生效"，而用户要做的
    /// 事完全不同。以前两种情况在界面上都没有任何症状。
    #[test]
    fn project_memory_info_tells_missing_from_truncated() {
        let _guard = crate::services::workspace::env_test_guard();
        let temp = std::env::temp_dir().join(format!("agent-ide-memory-info-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT_IDE_CONFIG_DIR", temp.join("config"));
        crate::services::workspace::save_workspace_path(temp.to_string_lossy().as_ref()).unwrap();

        let missing = project_memory_info().unwrap();
        assert!(!missing.exists);
        assert!(!missing.truncated);
        assert_eq!(missing.bytes, 0);
        assert!(missing.path.ends_with(PROJECT_MEMORY_FILE));
        // 不存在时给的是"新建"的提示词
        assert!(missing.draft_prompt.contains("no AGENTS.md yet"));

        std::fs::write(
            temp.join(PROJECT_MEMORY_FILE),
            "x".repeat(MAX_PROJECT_MEMORY_CHARS + 200),
        )
        .unwrap();
        let oversized = project_memory_info().unwrap();
        assert!(oversized.exists);
        assert!(
            oversized.truncated,
            "{} 字节应该算超出 {}",
            oversized.bytes, oversized.limit
        );
        // 已经存在时给的是"就地更新"的提示词，而且要说清为什么不能整份重写
        assert!(oversized.draft_prompt.contains("update it in place"));
        assert!(oversized.draft_prompt.contains("wholesale"));

        let _ = std::fs::remove_dir_all(temp);
    }

    /// 提示词里那三条约束是这份文件成败的全部，少一条都要能被这条测试抓住。
    #[test]
    fn the_draft_prompt_names_the_constraints_that_matter() {
        let prompt = draft_project_memory_prompt(false, 0);

        // 只写查得到的事实：编出来的构建命令比没有文档更糟
        assert!(prompt.contains("Only facts you verified"), "{}", prompt);
        assert!(prompt.contains("invented build command"), "{}", prompt);
        // 有上限，而且要说清超了会发生什么（静默丢尾部）
        assert!(
            prompt.contains(&MAX_PROJECT_MEMORY_CHARS.to_string()),
            "{}",
            prompt
        );
        assert!(prompt.contains("silently drops"), "{}", prompt);
        // 不要把这台机器上的绝对路径和密钥写进去
        assert!(prompt.contains("No secrets"), "{}", prompt);
        // 走审查区，而不是直接落盘
        assert!(prompt.contains("review area"), "{}", prompt);
    }

    /// **这个仓库自己的** `AGENTS.md` 必须装得下，而且要留出余量。    ///
    /// 截断机制本身上面已经测过了 —— 但那只证明"超了会被切"，不证明"我们的那份没超"。
    /// 而这份文件被注入每一次 Agent 运行，被切掉的是**尾部**，也就是"决定写到哪里"那一节：
    /// 没有任何报错，只是从此每次运行都少看到几条规则。这条测试把那次静默变成一次红色。
    ///
    /// 留 500 字节余量而不是贴着 8 000：贴着上限的话，下一个人加一行就越界，而越界的症状
    /// 是看不见的。`AGENTS.md` 自己写的就是"keep it under ~7 500"，这里把那句话变成可执行的。
    #[test]
    fn this_repos_project_memory_fits_with_headroom() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(PROJECT_MEMORY_FILE);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读不到 {}: {}", path.display(), error));
        let bytes = content.trim().len();

        // 硬上限：超过就会被真的切掉
        assert!(
            bytes <= MAX_PROJECT_MEMORY_CHARS,
            "AGENTS.md 有 {} 字节，超过了 {} 的注入上限，尾部会被静默切掉",
            bytes,
            MAX_PROJECT_MEMORY_CHARS
        );
        // 余量：越界的症状看不见，所以在还看得见的时候就报
        let recommended = MAX_PROJECT_MEMORY_CHARS - 500;
        assert!(
            bytes <= recommended,
            "AGENTS.md 有 {} 字节，已经超过建议的 {}；该精简，而不是继续往上加",
            bytes,
            recommended
        );
    }
}
