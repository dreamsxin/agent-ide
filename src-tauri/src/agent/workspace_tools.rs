//! Agent 在运行中主动读取工作区的内置工具。
//!
//! 在这之前，Agent 只能看到一次运行开始时打包好的上下文（活动文件、选区、
//! 目录树摘要、git diff）。要改一个用户没有预先选中的文件，模型只能凭猜测
//! 写出 `ORIGINAL` 段 —— 这正是 "Could not find original content" 类应用失败
//! 的来源。现代 agent 的做法是把"读什么"交给模型自己决定，用工具调用去取。
//!
//! 这些工具刻意不走 MCP：
//! - MCP 工具的参数完全不受约束（见 SECURITY.md），而这里每个路径都过
//!   `resolve_existing`，并且拒绝凭据文件，与上下文出网过滤保持一致。
//! - 不需要用户配置任何外部进程就能用。

use crate::agent::executor::ToolInvoker;
use crate::services::llm_client::ToolDefinition;
use crate::services::workspace;
use async_trait::async_trait;
use std::path::Path;

/// 内置工作区工具的名字前缀，与 MCP 的 `mcp__` 前缀互不重叠，
/// 这样 `handles` 的判定是确定的。
pub const WORKSPACE_TOOL_PREFIX: &str = "workspace_";

pub const READ_FILE: &str = "workspace_read_file";
/// 把工作区里的一张图片附到工具结果上给模型看。只读，和 `READ_FILE` 同一条边界。
pub const READ_IMAGE: &str = "workspace_read_image";
pub const SEARCH_TEXT: &str = "workspace_search_text";
pub const LIST_FILES: &str = "workspace_list_files";
pub const RUN_COMMAND: &str = "workspace_run_command";
pub const WRITE_FILE: &str = "workspace_write_file";
pub const DELETE_FILE: &str = "workspace_delete_file";
pub const MOVE_FILE: &str = "workspace_move_file";
pub const BROWSER_OPEN: &str = "workspace_browser_open";
pub const BROWSER_TABS: &str = "workspace_browser_tabs";
/// 枚举桌面上可见的顶层窗口。只读，但会披露窗口标题，所以同样按应用授权并留痕。
pub const COMPUTER_WINDOWS: &str = "workspace_computer_windows";

/// 单个文件最多回传的字节数，避免一次调用就吃掉整个上下文预算
const MAX_READ_BYTES: usize = 64_000;
/// 搜索最多回传的匹配行数
const MAX_SEARCH_RESULTS: usize = 60;
/// 列目录最多回传的条目数
const MAX_LIST_ENTRIES: usize = 200;
/// 命令输出回传给模型的字符上限。保尾部：报错在末尾
const MAX_COMMAND_OUTPUT_CHARS: usize = 12_000;
/// 遍历时跳过的目录：构建产物和依赖树，不是源码
const SKIPPED_DIRS: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".agent-ide",
];

/// Agent 通过写入工具落下的一次改动。
///
/// 记写之前的内容而不是只记路径：撤销要靠它，事后合成的 diff 卡片也要靠它
/// 才能显示"改了什么"，而不是只显示"改过"。
#[derive(Clone, Debug)]
pub struct AgentFileWrite {
    /// 工作区相对路径
    pub file: String,
    pub path: std::path::PathBuf,
    /// 写之前的内容，None 表示这是新建文件
    pub previous: Option<String>,
    pub updated: String,
    /// 这条记录是一次删除。
    ///
    /// 光看 `updated == ""` 分不出"删掉了"和"清空了"，而这两件事在审查卡片上必须
    /// 长得不一样 —— 把删除显示成"文件被清空"会让用户以为文件还在。撤销侧不需要
    /// 区分：`previous` 有内容就写回去，恰好就是重建那个文件。
    pub removed: bool,
    /// 这条记录是一次移动的落点，值是移动前的位置。
    ///
    /// 移动不能拆成"删一条 + 建一条"两条记录：那样审查区会出现两张互不相干的卡片，
    /// 各自的 rationale 还会指向根本没被调用的工具；而且两条记录之间没有原子性，
    /// 大小写不敏感的文件系统上 `Foo.ts` → `foo.ts` 更会变成两条指向同一个文件的
    /// 记录，撤销时先写回源、再删掉它，净结果是文件消失。
    pub moved_from: Option<MovedFrom>,
}

/// 移动前的位置：相对路径用于显示，绝对路径用于撤销时移回去
#[derive(Clone, Debug)]
pub struct MovedFrom {
    pub file: String,
    pub path: std::path::PathBuf,
}

type AgentWriteLog = std::sync::Arc<std::sync::Mutex<Vec<AgentFileWrite>>>;

/// 一次运行里内置工具的授权范围。
///
/// 只读工具无条件启用（受工作区边界约束、拒绝凭据文件）。命令执行和写入不一样：
/// 它们是真正的副作用，未授权时这些工具**根本不出现在模型的工具列表里** ——
/// 而不是出现之后再拒绝。让模型看见一个永远会失败的工具只会浪费轮次。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceToolPermissions {
    /// 允许执行的命令，支持 `cargo *` 前缀通配。空 = 不暴露命令执行工具
    pub allowed_commands: Vec<String>,
    /// 是否允许运行途中直接写盘。
    ///
    /// 只有 Auto 模式给。理由是它不构成新的特权等级：Auto 本来就在流水线结束
    /// 后自动落盘，不需要人点一下。Suggest/Edit 的产品约定是"人先看再落盘"，
    /// 那两种模式下这个工具不通告，模型照旧输出可审查的 diff。
    pub allow_write: bool,
    /// 是否允许新建文件（对应 `allowFileCreate`）。false 时只能改已存在的文件，
    /// 与 Auto 模式自动应用时对新建文件的处理保持一致。
    pub allow_create: bool,
    /// 是否允许驱动浏览器。和写盘分开：打开一个页面不改工作区，但它会把工作区里的
    /// 内容送到一个网站去，是另一种权限。
    pub allow_browser: bool,
    /// 允许访问的 origin 清单（`scheme://host[:port]`，`*` 表示不限）。
    ///
    /// 空清单等于不许访问任何站点，`allow_browser` 也救不了 —— 两者是"能不能用浏览器"
    /// 和"能去哪些站点"两个问题，任何一个没给都不该放行。
    pub browser_origins: Vec<String>,
    /// 是否允许观察桌面（目前只有窗口枚举，只读）。
    ///
    /// 和浏览器分开：浏览器权限的范围是"哪些站点"，这里的范围是"哪些应用"，两个问题
    /// 没有蕴含关系 —— 放行本地开发服务器不等于同意让模型看见桌面上开着什么。
    pub allow_computer: bool,
    /// 允许被观察的应用清单（可执行文件名，`*` 表示不限）。
    ///
    /// 空清单等于不许观察任何应用。它过滤的是**结果**，不只是决定工具存不存在 ——
    /// 浏览器 tabs 工具当初只做了后者，于是"只放行了本地开发服务器"的用户还是把所有
    /// 标签页交出去了。窗口标题里有文档名、网页标题、聊天对象，同一类问题。
    pub computer_apps: Vec<String>,
    /// 这批授权属于哪一次运行。
    ///
    /// 在拿到执行权之后由命令层写进来，而不是在登记记录时去读 orchestrator 的
    /// `current_run_id`：一次被 Stop 掉的运行可能在下一个 prompt 已经开跑之后才排空
    /// 它的记录，那时读到的是**后一次**运行的 id，于是前一次的导航被记在了后一次名下 ——
    /// 正好是这个字段本来要防的事。
    pub run_id: Option<String>,
    /// 这次运行的取消开关，**副作用**的那道闸门。
    ///
    /// 取消原本只拦得住模型调用和两次工具调用之间的间隙：一旦一次调用已经进到工具里，
    /// Stop 就管不着它了 —— 界面变空闲，而命令还在跑、页面还在被打开。撤不回的动作
    /// 尤其不能这样，所以有副作用的工具在动手之前先看这个开关。
    ///
    /// 和 `RunLease.cancel` 是**同一个** `Arc`（由命令层在拿执行权时交进去），不是第二
    /// 份状态。每次运行一个新的开关，从不复位旧的 —— 复位会把还在排空的旧运行解除取消。
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 已经发生的写入。跟着 `Clone` 共享同一份（`Arc`），所以命令层可以克隆一份
    /// 交给工具、另一份记在 orchestrator 上，事后从任一份都取得到记录。
    writes: AgentWriteLog,
    /// 已经发生的、**撤不回**的外部动作（目前只有浏览器）。
    ///
    /// 单独一份而不是塞进 `writes`：文件写入有 `previous` 可以还原，导航没有。混在
    /// 一起会让"撤销"这个词在同一个列表里有两种意思，而其中一种是假的。
    external: AgentExternalLog,
    /// 本次工具调用产生的、要随工具结果一起发给模型的图片。
    ///
    /// 又是一份单独的日志，理由和 `external` 一样：它和写入、和外部动作都不是同一种
    /// 东西 —— 图片既不需要撤销，也不是"已经发生的副作用"，它只是这次调用的返回值里
    /// 文本装不下的那部分。执行器在拼 `role: "tool"` 消息时排空它。
    images: AgentImageLog,
    /// 这次运行到目前为止已经附上的原始图片字节数。
    ///
    /// 单独一个计数器而不是数 `images` 的长度：`images` 每轮都被 `take_images` 排空，
    /// 拿它做预算等于每轮重新开始，而钱是按整次运行付的。跟着 `Clone` 共享同一个
    /// `Arc`，每次运行一份新的（授权对象本身就是每次运行新建的）。
    image_bytes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}


type AgentImageLog = std::sync::Arc<std::sync::Mutex<Vec<crate::services::images::ImagePart>>>;

/// 一次撤不回的外部动作。记录是这里唯一能承诺的东西，所以它必须完整到能复盘。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentExternalAction {
    /// 动作类别，例如 `browser_open`
    pub kind: String,
    /// 作用对象；浏览器动作是 URL 或 origin
    pub target: String,
    /// 结果摘要，成功和失败都记
    pub detail: String,
}

type AgentExternalLog = std::sync::Arc<std::sync::Mutex<Vec<AgentExternalAction>>>;

impl WorkspaceToolPermissions {
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn with_commands(allowed_commands: Vec<String>) -> Self {
        Self {
            allowed_commands,
            ..Self::default()
        }
    }

    pub fn new(allowed_commands: Vec<String>, allow_write: bool, allow_create: bool) -> Self {
        Self {
            allowed_commands,
            allow_write,
            allow_create,
            ..Self::default()
        }
    }

    /// 浏览器授权单独给，而不是加进 `new` 的参数表：调用点已经有十几处，多一个位置
    /// 参数只会让"这个 true 是哪个权限"变成读代码时的猜谜。
    pub fn with_browser(mut self, allow_browser: bool, browser_origins: Vec<String>) -> Self {
        self.allow_browser = allow_browser;
        self.browser_origins = browser_origins;
        self
    }

    /// 桌面观察授权，同理单独给。
    pub fn with_computer(mut self, allow_computer: bool, computer_apps: Vec<String>) -> Self {
        self.allow_computer = allow_computer;
        self.computer_apps = computer_apps;
        self
    }


    /// 取出并清空外部动作记录。
    pub fn take_external_actions(&self) -> Vec<AgentExternalAction> {
        match self.external.lock() {
            Ok(mut actions) => std::mem::take(&mut *actions),
            Err(_) => Vec::new(),
        }
    }

    fn record_external(&self, action: AgentExternalAction) {
        if let Ok(mut actions) = self.external.lock() {
            actions.push(action);
        }
    }

    /// 取走这次工具调用产生的图片。执行器每次 `invoke` 之后都调一次。
    pub fn take_images(&self) -> Vec<crate::services::images::ImagePart> {
        match self.images.lock() {
            Ok(mut images) => std::mem::take(&mut *images),
            Err(_) => Vec::new(),
        }
    }

    fn record_image(&self, image: crate::services::images::ImagePart) {
        if let Ok(mut images) = self.images.lock() {
            images.push(image);
        }
    }

    /// 这次运行到现在附了多少字节的图。
    pub fn images_bytes_used(&self) -> usize {
        self.image_bytes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 记上这一张图的字节数。超预算就拒绝，且**不**记账。
    ///
    /// 检查和累加放在一起，是为了不给"先检查后累加"留出两次调用之间的窗口。
    fn charge_image_bytes(&self, len: usize) -> Result<(), String> {
        let used = self.images_bytes_used();
        crate::services::images::check_run_image_budget(used, len)?;
        self.image_bytes
            .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }



    /// 浏览器工具是否可用：开关和清单都要有。
    fn allows_browser(&self) -> bool {
        self.allow_browser && !self.browser_origins.is_empty()
    }

    /// 桌面观察是否可用：开关、非空应用清单，以及这个平台上真的有实现。
    ///
    /// 平台也算一道条件：在没有实现的平台上通告一个必然失败的工具，只会让模型
    /// 反复调它、并把失败当成"桌面上没有窗口"。
    fn allows_computer(&self) -> bool {
        cfg!(windows) && self.allow_computer && !self.computer_apps.is_empty()
    }

    /// 接过这次运行的副作用开关。
    ///
    /// 开关由命令层在装任何工具面**之前**造出来，然后交给三个地方：这份授权、MCP 执行器、
    /// `try_begin_run`（进 `RunLease` 和 `CancelRegistry`）。三处同一个 `Arc`，不是三份
    /// 状态 —— 同步三份布尔值的版本正是"第二份状态"那类缺陷。
    ///
    /// 每次运行造一个新的，从不复位旧的：复位会把还在排空的旧运行解除取消。
    pub fn adopt_cancel(&mut self, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.cancel = cancel;
    }

    /// 这次运行是否已经被取消。有副作用的工具动手前问它。
    pub fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 把开关交给需要在执行途中反复检查它的工具（目前只有命令执行：它要靠这个杀子进程）。
    fn cancel_switch(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cancel.clone()
    }

    /// 取出并清空写入记录。
    ///
    /// 锁中毒时返回空而不是 panic：丢掉审计记录已经够糟，再让整次运行崩掉
    /// 是把一个可恢复的问题变成不可恢复的。
    pub fn take_writes(&self) -> Vec<AgentFileWrite> {
        match self.writes.lock() {
            Ok(mut writes) => std::mem::take(&mut *writes),
            Err(_) => Vec::new(),
        }
    }

    fn allows_commands(&self) -> bool {
        !self.allowed_commands.is_empty()
    }

    fn record_write(&self, write: AgentFileWrite) {
        if let Ok(mut writes) = self.writes.lock() {
            writes.push(write);
        }
    }
}

pub fn tool_definitions(permissions: &WorkspaceToolPermissions) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: READ_IMAGE.to_string(),
            description:
                "Look at an image file in the workspace (png, jpg, gif, webp). Use it when the task \
                 refers to a mockup, a diagram or a screenshot that is committed to the repo. The \
                 image is attached to the tool result; if the configured model cannot read images, \
                 the result says so instead."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path, e.g. docs/mockup.png"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: READ_FILE.to_string(),
            description:
                "Read a UTF-8 text file from the workspace. Use this before proposing an edit so \
                 the ORIGINAL section matches the file exactly. Paths are relative to the \
                 workspace root."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path, e.g. src/app.ts"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: SEARCH_TEXT.to_string(),
            description:
                "Search workspace file contents for a literal substring and return matching \
                 file paths with line numbers. Use this to locate the code you need to change."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Literal substring to look for (not a regular expression)"
                    },
                    "extension": {
                        "type": "string",
                        "description": "Optional file extension filter without the dot, e.g. ts"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: LIST_FILES.to_string(),
            description: "List files and directories under a workspace directory, one level deep."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative directory, defaults to the workspace root"
                    }
                }
            }),
        },
    ];

    if permissions.allow_write {
        definitions.push(ToolDefinition {
            name: WRITE_FILE.to_string(),
            description: format!(
                "Write the full new contents of a workspace file, then verify the result with a \
                 check command. Read the file first so you preserve everything you are not \
                 changing — this replaces the whole file, it does not patch it. {} The change is \
                 recorded as a reviewable, undoable entry, so prefer this over describing an edit \
                 you cannot verify.",
                if permissions.allow_create {
                    "New files may be created."
                } else {
                    "Only files that already exist may be written; creating new files is not \
                     permitted in this run."
                }
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path, e.g. src/app.ts"
                    },
                    "content": {
                        "type": "string",
                        "description": "Complete new file contents, not a patch or excerpt"
                    }
                },
                "required": ["path", "content"]
            }),
        });

        // 删除和写入用同一个授权位。理由是它们的后果同级 —— 整文件覆盖已经能把内容
        // 全部抹掉，再单独给删除加一道开关只是让人误以为写入更安全。
        //
        // 为什么值得内置：Agent 今天**根本删不掉文件**。`workspace_run_command` 只认
        // 项目自己声明的任务命令，`del` / `rm` 都不在里面；就算在，`rm -rf` 和
        // `del /q` 也不是同一个命令，跨平台得由我们来抹平，而不是让模型去猜操作系统。
        definitions.push(ToolDefinition {
            name: DELETE_FILE.to_string(),
            description: "Delete one workspace file. Use this for files that should no longer \
                          exist — do not empty a file to fake a deletion. Directories are not \
                          accepted. The removal is recorded as a reviewable, undoable entry, and \
                          undo puts the file back with its exact previous contents."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path of the file to delete"
                    }
                },
                "required": ["path"]
            }),
        });

        // 只在 allow_create 也给了的时候通告：移动会产生一个此前不存在的路径，没有这个
        // 授权它必然失败，而通告一个总是失败的工具比不通告更糟。
        if permissions.allow_create {
            definitions.push(ToolDefinition {
                name: MOVE_FILE.to_string(),
                description: "Move or rename one workspace file. Use this instead of writing the \
                              content to a new path and deleting the old one — that leaves the \
                              file duplicated if the second step fails, and the review entry \
                              would not say the file moved. Directories are not accepted, an \
                              existing destination is refused rather than overwritten, and the \
                              move is recorded as a reviewable entry whose undo puts the file \
                              back at its original path."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "from": {
                            "type": "string",
                            "description": "Workspace-relative path of the file to move"
                        },
                        "to": {
                            "type": "string",
                            "description": "Workspace-relative destination path, including the file name"
                        }
                    },
                    "required": ["from", "to"]
                }),
            });
        }
    }


    if permissions.allows_browser() {
        // 通告里就把清单写出来：模型看不到授权范围时只会不断试探被拒的站点，把轮次
        // 浪费在注定失败的调用上。
        definitions.push(ToolDefinition {
            name: BROWSER_OPEN.to_string(),
            description: format!(
                "Open a page in the user's Chrome (attached over the DevTools protocol) and \
                 bring it to the front. Allowed origins for this run: {}. A navigation cannot be \
                 undone — it is recorded in the run's action log instead. Use it to look at a \
                 local preview or a documentation page, not to submit forms or log in.",
                permissions.browser_origins.join(", ")
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Absolute http:// or https:// URL within an allowed origin"
                    }
                },
                "required": ["url"]
            }),
        });
        definitions.push(ToolDefinition {
            name: BROWSER_TABS.to_string(),
            description: "List the pages currently open in the attached Chrome, with their \
                          titles and URLs. Read-only."
                .to_string(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        });
    }

    if permissions.allows_computer() {
        definitions.push(ToolDefinition {
            name: COMPUTER_WINDOWS.to_string(),
            description: format!(
                "List the visible top-level desktop windows, with title, app, size and which one \
                 is in the foreground. Read-only; nothing on the desktop is changed. Only windows \
                 belonging to these apps are returned, and the count of hidden ones is reported: \
                 {}. Use it to see what the user is actually looking at, for example which editor \
                 or terminal window is in front.",
                permissions.computer_apps.join(", ")
            ),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        });
    }


    if permissions.allows_commands() {
        definitions.push(ToolDefinition {
            name: RUN_COMMAND.to_string(),
            description: format!(
                "Run one of the project's own check commands in the workspace root and return its \
                 exit code and output. Use this to see whether a change actually works before \
                 proposing it, and to read real failure output instead of guessing. Only these \
                 commands are permitted: {}. The command must exit on its own; dev servers and \
                 watch tasks are refused.",
                permissions.allowed_commands.join(", ")
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Exact command to run, e.g. npm test"
                    }
                },
                "required": ["command"]
            }),
        });
    }

    definitions
}

/// 工具调用日志回调：`(level, summary, details)`。
///
/// agent 层不认识 Tauri —— 发事件是命令层的事。之前这里直接持有 `AppHandle`，
/// 结果把 Tauri runtime 拖进了 lib 测试二进制，整个测试套件在加载时就
/// STATUS_ENTRYPOINT_NOT_FOUND 起不来。用回调把边界划回去，和 orchestrator
/// 只做状态、命令层负责发事件的分法一致。
pub type ToolCallLogger = std::sync::Arc<dyn Fn(&str, &str, &str) + Send + Sync>;

pub struct WorkspaceToolInvoker {
    logger: Option<ToolCallLogger>,
    permissions: WorkspaceToolPermissions,
}

impl WorkspaceToolInvoker {
    pub fn new(logger: ToolCallLogger, permissions: WorkspaceToolPermissions) -> Self {
        Self {
            logger: Some(logger),
            permissions,
        }
    }

    /// 不写日志的构造方式（测试路径）
    pub fn without_logging(permissions: WorkspaceToolPermissions) -> Self {
        Self {
            logger: None,
            permissions,
        }
    }

    /// 每次工具调用都记一条。
    ///
    /// 不记的话这些工具是完全不透明的：Agent 读了哪些文件、搜了什么，用户
    /// 无从得知，而 MCP 工具是有记录的 —— 内置工具不该比外部工具更不可审计。
    fn log(&self, level: &str, summary: &str, details: &str) {
        if let Some(ref logger) = self.logger {
            logger(level, summary, details);
        }
    }
}

#[async_trait]
impl ToolInvoker for WorkspaceToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        match tool_name {
            READ_FILE | SEARCH_TEXT | LIST_FILES | READ_IMAGE => true,
            // 未授权时不认领：工具本来也没有被通告出去，认领它只会把一个
            // "不存在的工具"变成一个"总是失败的工具"
            RUN_COMMAND => self.permissions.allows_commands(),
            WRITE_FILE | DELETE_FILE => self.permissions.allow_write,
            BROWSER_OPEN | BROWSER_TABS => self.permissions.allows_browser(),
            COMPUTER_WINDOWS => self.permissions.allows_computer(),
            // 移动会产生一个新路径，两个授权都要有；缺一个的时候它没被通告，也就不认领
            MOVE_FILE => self.permissions.allow_write && self.permissions.allow_create,
            _ => false,
        }
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        let args: serde_json::Value = serde_json::from_str(arguments.trim())
            .map_err(|error| format!("Tool arguments are not valid JSON: {}", error))?;
        // 参数里只有路径和查询串，没有机密可言，可以原样记录
        self.log(
            "info",
            &format!("Agent called {}", tool_name),
            &format!("Arguments:\n{}", arguments.trim()),
        );
        // 副作用的闸门在这里，而不是在每个工具里各写一遍：取消原本只在两次工具调用
        // **之间**生效，一次已经进到这里的调用照旧会跑命令、照旧会打开页面 —— 用户点了
        // Stop，界面变空闲，世界还在被改。只读工具放过：它们不改变任何东西，拒掉只会
        // 给一份马上要丢掉的对话再添噪声。
        if self.permissions.cancelled() {
            let side_effecting = matches!(
                tool_name,
                RUN_COMMAND
                    | WRITE_FILE
                    | DELETE_FILE
                    | MOVE_FILE
                    | BROWSER_OPEN
                    | BROWSER_TABS
                    | COMPUTER_WINDOWS
            );
            if side_effecting {
                let detail = format!(
                    "This run was stopped, so {} was refused before it could take effect.",
                    tool_name
                );
                // 浏览器和桌面观察的尝试仍然进外部动作日志：它没有发生，但"停了之后
                // 模型还想出网 / 还想读窗口标题"是用户会想知道的事。上一版只记了浏览器，
                // 桌面那条就悄悄只剩一行普通日志 —— 加了新工具没检查记录侧的老毛病。
                if matches!(tool_name, BROWSER_OPEN | BROWSER_TABS | COMPUTER_WINDOWS) {
                    self.permissions.record_external(AgentExternalAction {
                        kind: format!("{}_cancelled", tool_name.trim_start_matches("workspace_")),
                        target: string_arg(&args, "url")
                            .unwrap_or(if tool_name == COMPUTER_WINDOWS {
                                "desktop"
                            } else {
                                "chrome"
                            })
                            .to_string(),
                        detail: detail.clone(),
                    });
                }
                self.log("warn", &format!("Refused {} after Stop", tool_name), &detail);
                return Err(detail);
            }
        }
        let result = match tool_name {
            READ_FILE => read_file_tool(string_arg(&args, "path").ok_or("Missing 'path'")?),
            READ_IMAGE => read_image_tool(
                string_arg(&args, "path").ok_or("Missing 'path'")?,
                &self.permissions,
            ),
            SEARCH_TEXT => search_text_tool(
                string_arg(&args, "query").ok_or("Missing 'query'")?,
                string_arg(&args, "extension"),
            ),
            LIST_FILES => list_files_tool(string_arg(&args, "path").unwrap_or(".")),
            RUN_COMMAND => {
                run_command_tool(
                    string_arg(&args, "command").ok_or("Missing 'command'")?,
                    &self.permissions.allowed_commands,
                    self.permissions.cancel_switch(),
                )
                .await
            }
            WRITE_FILE => write_file_tool(
                string_arg(&args, "path").ok_or("Missing 'path'")?,
                args.get("content")
                    .and_then(|value| value.as_str())
                    .ok_or("Missing 'content'")?,
                &self.permissions,
            ),
            DELETE_FILE => delete_file_tool(
                string_arg(&args, "path").ok_or("Missing 'path'")?,
                &self.permissions,
            ),
            MOVE_FILE => move_file_tool(
                string_arg(&args, "from").ok_or("Missing 'from'")?,
                string_arg(&args, "to").ok_or("Missing 'to'")?,
                &self.permissions,
            ),
            BROWSER_OPEN => browser_open_tool(
                string_arg(&args, "url").ok_or("Missing 'url'")?,
                &self.permissions,
            ),
            BROWSER_TABS => browser_tabs_tool(&self.permissions),
            COMPUTER_WINDOWS => computer_windows_tool(&self.permissions),
            other => Err(format!("Unknown workspace tool: {}", other)),
        };
        match &result {
            Ok(output) => self.log(
                "success",
                &format!("{} returned {} chars", tool_name, output.len()),
                output,
            ),
            Err(error) => self.log("warn", &format!("{} failed", tool_name), error),
        }
        result
    }

    /// 把 `read_image_tool` 挂上来的图片交给执行器。
    fn take_images(&self) -> Vec<crate::services::images::ImagePart> {
        self.permissions.take_images()
    }
}

fn string_arg<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 凭据文件对 Agent 一律不可读。
///
/// 上下文构建那边已经把 `.env` 的内容挡掉了，如果这里放行，等于给模型开了
/// 一条绕过出网过滤的后门 —— 它只要主动调一次读取工具就能拿到同样的内容。
fn reject_credential_path(path: &str) -> Result<(), String> {
    if workspace::is_credential_path(path) {
        return Err(format!(
            "Reading {} is not allowed: it looks like a credential file",
            path
        ));
    }
    Ok(())
}

fn read_file_tool(path: &str) -> Result<String, String> {
    reject_credential_path(path)?;
    let resolved = workspace::resolve_existing(path)?;
    // 解析后再判一次：`docs/notes.md -> ../.env` 这样的符号链接，名字过得了第一道，
    // 而 `resolve_existing` 会跟随它。两道判定之间的差就是这条后门。
    reject_credential_path(&resolved.to_string_lossy())?;
    let content =
        std::fs::read_to_string(&resolved).map_err(|error| format!("Read {}: {}", path, error))?;
    if content.len() <= MAX_READ_BYTES {
        return Ok(content);
    }
    let head: String = content.chars().take(MAX_READ_BYTES).collect();
    Ok(format!(
        "{}\n... [truncated at {} bytes; read a narrower range or search instead]",
        head, MAX_READ_BYTES
    ))
}

/// 让模型看工作区里的一张图片。
///
/// 这是多模态这条线上的第一个生产者，选它的理由是代价：图片已经在工作区里，走的是和
/// `workspace_read_file` 同一条解析和拒绝规则，不需要任何新的 OS 权限，也不需要编码器。
///
/// **拒绝判定落在解析后的目标上。** 只判调用方写的那个字符串是不够的：仓库里可以提交一个
/// `docs/mockup.png -> ../.env` 的符号链接，名字看着是图片、`resolve_existing` 又会跟随
/// 链接，于是 `.env` 的字节被 base64 送去了模型那边。这条工具的出口比 `read_file` 宽
/// （4 MiB vs 64 KB），而且日志里只留一句"附了一张图"，所以这里必须判两次。
///
/// 大小在**读之前**用 metadata 判：先把两个 GB 的 PNG 读进内存再说"超限"，就是给一个
/// 模型能反复调用的只读工具留了一条内存耗尽的路。
fn read_image_tool(path: &str, permissions: &WorkspaceToolPermissions) -> Result<String, String> {
    reject_credential_path(path)?;
    let resolved = workspace::resolve_existing(path)?;
    reject_credential_path(&resolved.to_string_lossy())?;
    let size = std::fs::metadata(&resolved)
        .map_err(|error| format!("Read {}: {}", path, error))?
        .len();
    if size > crate::services::images::MAX_IMAGE_BYTES as u64 {
        return Err(format!(
            "{} is {} bytes, over the {} byte limit for one image.",
            path,
            size,
            crate::services::images::MAX_IMAGE_BYTES
        ));
    }
    // 预算在**读之前**就问一次：超了的图连读进内存都不必，理由和单张上限用
    // `metadata` 先量一遍一样。
    crate::services::images::check_run_image_budget(
        permissions.images_bytes_used(),
        size as usize,
    )?;
    let bytes = std::fs::read(&resolved).map_err(|error| format!("Read {}: {}", path, error))?;
    let image = crate::services::images::image_part_from_bytes(path, &bytes)?;
    // 记账放在解析成功之后：类型不认或文件是空的那些请求根本没出网，不该占预算。
    // 检查在读之前也做过一次（见上），所以超预算的图从来不会被整个读进内存。
    permissions.charge_image_bytes(bytes.len())?;
    let media_type = image.media_type.clone();
    permissions.record_image(image);
    // 文本里写解析后的路径：事后复盘时"哪张图出去了"要对得上磁盘上的文件，
    // 而不是模型当时写的那个可能是链接的名字
    Ok(format!(
        "Attached {} ({}, {} bytes) to this turn. Resolved to {}.",
        path,
        media_type,
        bytes.len(),
        resolved.display()
    ))
}

fn list_files_tool(path: &str) -> Result<String, String> {
    let resolved = workspace::resolve_existing(path)?;
    let mut entries: Vec<String> = Vec::new();
    let read_dir =
        std::fs::read_dir(&resolved).map_err(|error| format!("List {}: {}", path, error))?;
    for entry in read_dir.flatten() {
        if entries.len() >= MAX_LIST_ENTRIES {
            entries.push("... [truncated]".to_string());
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIPPED_DIRS.contains(&name.as_str()) {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        entries.push(if is_dir { format!("{}/", name) } else { name });
    }
    entries.sort();
    if entries.is_empty() {
        return Ok(format!("{} is empty", path));
    }
    Ok(entries.join("\n"))
}

fn search_text_tool(query: &str, extension: Option<&str>) -> Result<String, String> {
    let root = workspace::workspace_root()?;
    let mut matches: Vec<String> = Vec::new();
    walk_and_match(&root, &root, query, extension, &mut matches);
    if matches.is_empty() {
        return Ok(format!("No matches for {:?}", query));
    }
    let truncated = matches.len() > MAX_SEARCH_RESULTS;
    matches.truncate(MAX_SEARCH_RESULTS);
    if truncated {
        matches.push("... [more matches omitted; narrow the query]".to_string());
    }
    Ok(matches.join("\n"))
}

fn walk_and_match(
    root: &Path,
    dir: &Path,
    query: &str,
    extension: Option<&str>,
    matches: &mut Vec<String>,
) {
    if matches.len() > MAX_SEARCH_RESULTS {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        if matches.len() > MAX_SEARCH_RESULTS {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk_and_match(root, &path, query, extension, matches);
            }
            continue;
        }
        // 凭据文件不出现在搜索结果里，和读取工具、上下文过滤保持一致
        if workspace::is_credential_path(&name) {
            continue;
        }
        if let Some(extension) = extension {
            if path.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
        }
        // 二进制文件读不成 UTF-8，直接跳过而不是报错
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative = relative.to_string_lossy().replace('\\', "/");
        for (index, line) in content.lines().enumerate() {
            if line.contains(query) {
                matches.push(format!("{}:{}: {}", relative, index + 1, line.trim()));
                if matches.len() > MAX_SEARCH_RESULTS {
                    return;
                }
            }
        }
    }
}

/// 跑一条项目自己的检查命令，把退出码和输出回给模型。
///
/// 这是"闭环"缺的那一半：在此之前模型只能提出一个 diff，然后永远看不到结果，
/// 失败原因要等用户手动点一次 Verify 再贴回来。
///
/// 三重约束，顺序有意为之：
/// 1. 长驻命令一律拒绝。验证是跑完再看结果，`npm run dev` 永远不退出 ——
///    这是安全不变量，不由允许清单覆盖，所以先判它。
/// 2. 必须命中允许清单。清单由后端从项目自己声明的任务里推导，不是模型自选。
/// 3. 输出保尾部截断：报错在末尾，保头部等于只把编译进度喂给模型。
async fn run_command_tool(
    command: &str,
    allowed: &[String],
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<String, String> {
    use crate::services::verification;

    if verification::is_long_running_command(command) {
        return Err(format!(
            "Refusing to run {:?}: it looks like a long-running command (dev server or watch \
             task) and would never exit. Run a check that terminates, such as a test or build \
             command.",
            command
        ));
    }
    if !verification::is_command_allowed(command, allowed) {
        return Err(format!(
            "Command {:?} is not authorized for this run. Allowed: {}",
            command,
            allowed.join(", ")
        ));
    }

    let root = workspace::workspace_root()?;
    let result = crate::services::project_tasks::run_project_command_cancellable(
        command.to_string(),
        root,
        cancel,
    )
    .await?;
    let output = [result.stdout.as_str(), result.stderr.as_str()]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let exit = result
        .exit_code
        .map(|code| code.to_string())
        // 退出码拿不到不等于成功：命令没能正常结束就是没结束
        .unwrap_or_else(|| "unknown (command did not report an exit code)".to_string());

    Ok(format!(
        "$ {}\nexit code: {}\nduration: {} ms\nproblems parsed: {}\n\n{}",
        result.command,
        exit,
        result.duration_ms,
        result.problems.len(),
        if output.trim().is_empty() {
            "(no output)".to_string()
        } else {
            verification::truncate_for_prompt(&output, MAX_COMMAND_OUTPUT_CHARS)
        }
    ))
}

/// 把整份新内容写进工作区文件。
///
/// 这是闭环的最后一半：在此之前模型能读、能跑检查，但改动只能以 `agent-changes`
/// 输出交给人应用，所以它永远看不到**自己那次改动**之后的状态。
///
/// 为什么整份覆盖而不是打补丁：补丁定位失败（"Could not find original content"）
/// 是既有失败模式的主要来源，而模型已经有读取工具可以先取到准确内容。整份写入
/// 把"定位"这一步彻底去掉。代价是模型必须保留它不想改的部分，工具描述里明说了。
///
/// 约束：
/// - 路径过 `resolve_for_agent_write`，因此 `.git/`、`.agent-ide/`、`node_modules/`
///   和凭据文件一律拒绝——和 diff 应用路径同一套规则，不是另写一份。
/// - 新建文件需要 `allow_create`，与 Auto 模式自动应用时对新建文件的处理一致。
/// - 内容没变时不记录：给撤销栈塞一个什么都没改的回滚点会让栈顶失真。
fn write_file_tool(
    path: &str,
    content: &str,
    permissions: &WorkspaceToolPermissions,
) -> Result<String, String> {
    if !permissions.allow_write {
        return Err(
            "Writing files is not authorized for this run. Return an agent-changes block for \
             review instead."
                .to_string(),
        );
    }
    let resolved = workspace::resolve_for_agent_write(path)?;
    let previous = match std::fs::read_to_string(&resolved) {
        Ok(existing) => Some(existing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Read {} before writing: {}", path, error)),
    };
    if previous.is_none() && !permissions.allow_create {
        return Err(format!(
            "{} does not exist and creating files is not authorized for this run.",
            path
        ));
    }
    if previous.as_deref() == Some(content) {
        return Ok(format!(
            "{} already has this exact content; nothing written.",
            path
        ));
    }

    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Create parent directory for {}: {}", path, error))?;
    }
    std::fs::write(&resolved, content).map_err(|error| format!("Write {}: {}", path, error))?;

    permissions.record_write(AgentFileWrite {
        file: path.to_string(),
        path: resolved,
        previous: previous.clone(),
        updated: content.to_string(),
        removed: false,
        moved_from: None,
    });

    Ok(format!(
        "{} {} ({} bytes). The change is recorded and can be undone.",
        if previous.is_none() {
            "Created"
        } else {
            "Updated"
        },
        path,
        content.len()
    ))
}

/// 删除一个工作区文件，并留下可审查、可撤销的记录。
///
/// 走的是和写入完全相同的边界：`resolve_for_agent_write` 既拦工作区外的路径，也拦
/// `.git/`、`.agent-ide/`、`node_modules/` 和凭据文件。删除比覆盖更不可逆，所以这里
/// 一条边界都不能比写入宽松。
///
/// 只删文件，不删目录。递归删除的爆炸半径完全不同 —— 一次说错的目录名可以清掉整个
/// 子树，而现在的撤销记录是"文件 → 内容"的列表，重建一棵目录树需要另一套东西。
/// 与其给一个半个撤销得回来的能力，不如明确拒绝。
fn delete_file_tool(path: &str, permissions: &WorkspaceToolPermissions) -> Result<String, String> {
    let resolved = workspace::resolve_for_agent_write(path)?;
    if resolved.is_dir() {
        return Err(format!(
            "{} is a directory; this tool deletes single files only.",
            path
        ));
    }
    // 先读内容再删：撤销就是把这份内容写回去，读不出来就没有可撤销的删除。
    let previous = match std::fs::read_to_string(&resolved) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{} does not exist.", path));
        }
        Err(error) => {
            return Err(format!(
                "Read {} before deleting it: {}. Refusing to delete something that cannot be \
                 restored.",
                path, error
            ));
        }
    };

    std::fs::remove_file(&resolved).map_err(|error| format!("Delete {}: {}", path, error))?;

    permissions.record_write(AgentFileWrite {
        file: path.to_string(),
        path: resolved,
        previous: Some(previous.clone()),
        updated: String::new(),
        removed: true,
        moved_from: None,
    });

    Ok(format!(
        "Deleted {} ({} bytes). The removal is recorded and can be undone.",
        path,
        previous.len()
    ))
}

/// 把一个文件移到另一个路径 —— 同一个操作也覆盖重命名。
///
/// 为什么必须内置：`workspace_run_command` 只认项目自己声明的检查命令，`mv` / `move`
/// 都不在其中；而且这两个命令在不同平台上语义并不一致，跨平台该由我们抹平，而不是
/// 让模型去猜操作系统。写 + 删两步也不行：中间失败就是内容留在磁盘上而记录只有一半。
///
/// 实现用 `fs::rename` 而不是"读内容 → 写到新路径 → 删旧路径"：
/// - 一次系统调用，不存在只做了一半的中间态；
/// - 不读内容，所以二进制文件也能移动。`delete_file` 受 `read_to_string` 限制只能处理
///   UTF-8 文本，是因为它的撤销要靠内容重建；移动的撤销是**移回去**，不需要内容。
///
/// 约束：
/// - 两端都过 `resolve_for_agent_write`，所以 `.git/`、`.agent-ide/`、凭据文件既不能当
///   源也不能当目标；
/// - 目标已存在时拒绝，不静默覆盖 —— 覆盖会连带毁掉目标原有的内容，而这一步没有任何
///   记录能撤销它；
/// - 目录拒绝，和 `delete_file` 一致；
/// - 需要 `allow_write`（源要消失）**和** `allow_create`（目标是一个此前不存在的路径）。
fn move_file_tool(
    from: &str,
    to: &str,
    permissions: &WorkspaceToolPermissions,
) -> Result<String, String> {
    if !permissions.allow_write {
        return Err(
            "Moving files is not authorized for this run. Return an agent-changes block for \
             review instead."
                .to_string(),
        );
    }
    if !permissions.allow_create {
        return Err(format!(
            "Moving {} to {} would create a new path and creating files is not authorized for \
             this run.",
            from, to
        ));
    }

    let source = workspace::resolve_for_agent_write(from)?;
    let destination = workspace::resolve_for_agent_write(to)?;
    if source == destination {
        // 只改大小写的重命名也落在这里：`resolve_for_write` 会 canonicalize 已存在的
        // 路径，而 Windows / macOS 返回的是磁盘上真实的大小写，于是 `foo.ts` 和
        // `Foo.ts` 解析成同一个 PathBuf。所以这条消息要直接给出可行的做法，而不是
        // 让模型反复重试同一个调用。
        return Err(format!(
            "{} and {} are the same path; nothing to move. A case-only rename needs an \
             intermediate name (move to a temporary path first, then to the final one).",
            from, to
        ));
    }
    if source.is_dir() {
        return Err(format!(
            "{} is a directory; this tool moves single files only.",
            from
        ));
    }
    if !source.exists() {
        return Err(format!("{} does not exist.", from));
    }
    // 目标存在就拒绝，一个例外都不留。之前这里放过"只差大小写"的情况，而大小写敏感性
    // 是**每个目录**的属性（Windows 10+ 的 setCaseSensitiveInfo、大小写敏感的 APFS
    // 卷），不是编译目标的属性 —— 在那些地方 `Foo.ts` 和 `foo.ts` 是两个真实文件，
    // 放行等于用 `fs::rename` 静默覆盖掉一个没有任何记录能恢复的文件。
    if destination.exists() {
        return Err(format!(
            "{} already exists. Refusing to overwrite it — delete it first if that is really \
             intended.",
            to
        ));
    }

    // 只在父目录确实不存在时创建，并记住是我们建的：`fs::rename` 仍然可能失败（跨卷、
    // 权限、Windows 上的共享冲突），那时候留下一棵空目录树等于一次没有任何记录、也无法
    // 撤销的工作区改动。
    let created_parent = match destination.parent() {
        Some(parent) if !parent.exists() => {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Create parent directory for {}: {}", to, error))?;
            Some(parent.to_path_buf())
        }
        _ => None,
    };
    if let Err(error) = std::fs::rename(&source, &destination) {
        if let Some(parent) = created_parent {
            // 只删空目录，remove_dir 天然不会碰有内容的目录
            let mut current = Some(parent);
            while let Some(directory) = current {
                if std::fs::remove_dir(&directory).is_err() {
                    break;
                }
                current = directory.parent().map(|path| path.to_path_buf());
            }
        }
        return Err(format!("Move {} to {}: {}", from, to, error));
    }

    permissions.record_write(AgentFileWrite {
        file: to.to_string(),
        path: destination,
        // 移动没有"之前的内容"：目标路径此前不存在，撤销靠移回去而不是靠内容
        previous: None,
        updated: String::new(),
        removed: false,
        moved_from: Some(MovedFrom {
            file: from.to_string(),
            path: source,
        }),
    });

    Ok(format!(
        "Moved {} to {}. The move is recorded and undo puts it back at {}.",
        from, to, from
    ))
}

/// 在同步的工具接口里跑一次异步请求。
///
/// `ToolInvoker::invoke` 是同步的（其他工具都是文件和进程操作），而浏览器走 HTTP。
/// `block_in_place` 把当前工作线程让出去，所以不会把整个多线程 runtime 堵死；没有
/// runtime 时（单元测试直接调用）如实说明，而不是 panic。
fn block_on_browser<T>(
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| "Browser tools need the app runtime; not available here.".to_string())?;
    tokio::task::block_in_place(|| handle.block_on(future))
}

/// 未授权就调用也要记一笔。
///
/// 这条路径原本只返回错误：审计里发现"模型在浏览器权限关着的时候还是调了浏览器工具"
/// 根本没进记录，而它和"调了但站点不在清单里"对用户是同一类信息 —— 都是模型想出网。
fn refuse_browser(
    kind: &str,
    target: &str,
    permissions: &WorkspaceToolPermissions,
) -> Result<String, String> {
    let detail = "Browser use is not authorized for this run, or no origin is allowed.".to_string();
    permissions.record_external(AgentExternalAction {
        kind: kind.to_string(),
        target: target.to_string(),
        detail: detail.clone(),
    });
    Err(detail)
}

/// 打开一个页面。
///
/// 两道闸门，缺一不可：`allow_browser`（能不能用浏览器）和 origin 清单（能去哪儿）。
/// 拒绝也要记进外部动作日志 —— "模型试图打开某个没授权的站点"正是用户事后最想知道的
/// 事情之一，只在返回值里说一句会随着这一轮对话消失。
fn browser_open_tool(
    url: &str,
    permissions: &WorkspaceToolPermissions,
) -> Result<String, String> {
    if !permissions.allows_browser() {
        return refuse_browser("browser_open_refused", url, permissions);
    }
    let origin = match crate::services::browser::origin_of(url) {
        Ok(origin) => origin,
        Err(error) => {
            permissions.record_external(AgentExternalAction {
                kind: "browser_open_refused".to_string(),
                target: url.to_string(),
                detail: error.clone(),
            });
            return Err(error);
        }
    };
    if !crate::services::browser::origin_allowed(&origin, &permissions.browser_origins) {
        let detail = format!(
            "{} is not in the allowed origins for this run ({}).",
            origin,
            permissions.browser_origins.join(", ")
        );
        permissions.record_external(AgentExternalAction {
            kind: "browser_open_refused".to_string(),
            target: origin,
            detail: detail.clone(),
        });
        return Err(detail);
    }

    let port = crate::services::browser::configured_port();
    match block_on_browser(crate::services::browser::open_url(port, url)) {
        Ok(tab) => {
            permissions.record_external(AgentExternalAction {
                kind: "browser_open".to_string(),
                target: tab.url.clone(),
                detail: format!("Opened \"{}\" (tab {})", tab.title, tab.id),
            });
            Ok(format!(
                "Opened {} in Chrome (title: {}). This navigation is recorded and cannot be undone.",
                tab.url, tab.title
            ))
        }
        Err(error) => {
            permissions.record_external(AgentExternalAction {
                kind: "browser_open_failed".to_string(),
                target: url.to_string(),
                detail: error.clone(),
            });
            Err(error)
        }
    }
}

/// 列出标签页。只读，但同样记录：它把用户所有打开页面的标题和 URL 交给了模型。
///
/// 记录里写出被披露的 origin 而不是只写一个数量：清单只决定这个工具是否存在，不限制
/// 结果 —— 只放行了本地开发服务器的用户，同样把内部站点、带 token 的回调 URL 交出去了，
/// 事后光看"列了 7 个页面"复盘不出泄了什么。
fn browser_tabs_tool(permissions: &WorkspaceToolPermissions) -> Result<String, String> {
    if !permissions.allows_browser() {
        return refuse_browser("browser_tabs_refused", "chrome", permissions);
    }
    let port = crate::services::browser::configured_port();
    let tabs = match block_on_browser(crate::services::browser::list_tabs(port)) {
        Ok(tabs) => tabs,
        Err(error) => {
            // 这里原来是 `?`：读工具的传输失败一条记录都没留下，而空记录会让
            // `publish_external_actions` 提前返回，整轮运行看起来什么都没发生过。
            permissions.record_external(AgentExternalAction {
                kind: "browser_tabs_failed".to_string(),
                target: format!("127.0.0.1:{}", port),
                detail: error.clone(),
            });
            return Err(error);
        }
    };
    let mut origins: Vec<String> = Vec::new();
    for tab in &tabs {
        if let Ok(origin) = crate::services::browser::origin_of(&tab.url) {
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
    }
    permissions.record_external(AgentExternalAction {
        kind: "browser_tabs".to_string(),
        target: format!("127.0.0.1:{}", port),
        detail: format!(
            "Disclosed the title and URL of {} open page(s) to the model. Origins: {}",
            tabs.len(),
            if origins.is_empty() {
                "(none)".to_string()
            } else {
                origins.join(", ")
            }
        ),
    });
    if tabs.is_empty() {
        return Ok("Chrome is attached but has no open pages.".to_string());
    }
    Ok(tabs
        .iter()
        .map(|tab| format!("- {} — {}", tab.title, tab.url))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// 列出桌面上可见的顶层窗口。computer use 的第一片，只读。
///
/// 两道闸门和浏览器同构：开关 + 非空的**应用**清单。清单在这里过滤的是结果，不只是
/// 决定工具存不存在 —— 窗口标题里有文档名、网页标题、聊天对象，"只放行了 VS Code"
/// 的用户不该顺带交出银行页面的标题。
///
/// 拒绝和成功都记进外部动作日志。这个工具不改变任何东西，但它披露的东西撤不回，
/// 记录的理由和导航一样。
fn computer_windows_tool(permissions: &WorkspaceToolPermissions) -> Result<String, String> {
    if !permissions.allows_computer() {
        let detail = if cfg!(windows) {
            "Desktop observation is not authorized for this run, or no app is allowed."
        } else {
            "Desktop observation is only implemented on Windows."
        };
        permissions.record_external(AgentExternalAction {
            kind: "computer_windows_refused".to_string(),
            target: "desktop".to_string(),
            detail: detail.to_string(),
        });
        return Err(detail.to_string());
    }
    let windows = match crate::services::computer::list_windows() {
        Ok(windows) => windows,
        Err(error) => {
            permissions.record_external(AgentExternalAction {
                kind: "computer_windows_failed".to_string(),
                target: "desktop".to_string(),
                detail: error.clone(),
            });
            return Err(error);
        }
    };
    let (allowed, hidden) =
        crate::services::computer::filter_windows(windows, &permissions.computer_apps);
    let apps = crate::services::computer::disclosed_apps(&allowed);
    permissions.record_external(AgentExternalAction {
        kind: "computer_windows".to_string(),
        target: "desktop".to_string(),
        detail: format!(
            "Disclosed the title and geometry of {} window(s) to the model ({} hidden by the allow list). Apps: {}",
            allowed.len(),
            hidden,
            if apps.is_empty() {
                "(none)".to_string()
            } else {
                apps.join(", ")
            }
        ),
    });
    Ok(crate::services::computer::format_windows(&allowed, hidden))
}


/// 把多个执行器合成一个。
///
/// `select_external_calls` 只接受一个 `Option<&dyn ToolInvoker>`，而一次运行里
/// 内置工作区工具和 MCP 工具都可能启用，所以这里按顺序找第一个认领该名字的。
pub struct CompositeToolInvoker {
    invokers: Vec<std::sync::Arc<dyn ToolInvoker>>,
}

impl CompositeToolInvoker {
    pub fn new(invokers: Vec<std::sync::Arc<dyn ToolInvoker>>) -> Self {
        Self { invokers }
    }
}

#[async_trait]
impl ToolInvoker for CompositeToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        self.invokers
            .iter()
            .any(|invoker| invoker.handles(tool_name))
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        for invoker in &self.invokers {
            if invoker.handles(tool_name) {
                return invoker.invoke(tool_name, arguments).await;
            }
        }
        Err(format!("No invoker handles tool {}", tool_name))
    }

    /// 向所有子执行器要图片。
    ///
    /// 不记住"上一次是谁处理的"：那是一份会和真相分叉的状态。只有产生了图片的那个
    /// 子执行器会返回非空，其余返回默认的空 vec。
    fn take_images(&self) -> Vec<crate::services::images::ImagePart> {
        self.invokers
            .iter()
            .flat_map(|invoker| invoker.take_images())
            .collect()
    }
}

/// 把内置工作区工具接到一次运行上，并与已有的（MCP）执行器合并。
///
/// 只读工具无条件启用：它们受工作区边界约束、且拒绝凭据文件，不像 MCP 那样
/// 需要用户先信任一个外部进程。命令执行按 `permissions` 决定是否暴露。
pub fn attach_workspace_tools(
    llm: crate::services::llm_client::LlmClient,
    existing: Option<std::sync::Arc<dyn ToolInvoker>>,
    logger: Option<ToolCallLogger>,
    permissions: WorkspaceToolPermissions,
) -> (
    crate::services::llm_client::LlmClient,
    Option<std::sync::Arc<dyn ToolInvoker>>,
) {
    let mut definitions = llm.extra_tools().to_vec();
    definitions.extend(tool_definitions(&permissions));

    let invoker = match logger {
        Some(logger) => WorkspaceToolInvoker::new(logger, permissions),
        None => WorkspaceToolInvoker::without_logging(permissions),
    };
    let mut invokers: Vec<std::sync::Arc<dyn ToolInvoker>> = vec![std::sync::Arc::new(invoker)];
    if let Some(existing) = existing {
        invokers.push(existing);
    }

    (
        llm.with_extra_tools(definitions),
        Some(std::sync::Arc::new(CompositeToolInvoker::new(invokers))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// 测试里"没有被取消"的开关。
    fn test_cancel() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    struct TestEnv {
        root: std::path::PathBuf,
        config_dir: std::path::PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("agent-ide-tools-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = workspace::shell_compatible_path(root.canonicalize().unwrap());
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write(&self, relative: &str, content: &str) {
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
            let _ = std::fs::remove_dir_all(self.root.parent().unwrap_or(&self.root));
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    #[test]
    fn read_file_returns_workspace_content_and_refuses_escapes() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "const value = 1;\n");

        assert_eq!(read_file_tool("src/app.ts").unwrap(), "const value = 1;\n");

        let err = read_file_tool("../outside.txt").unwrap_err();
        assert!(
            err.contains("outside workspace") || err.contains("does not exist"),
            "{}",
            err
        );
    }

    /// 上下文构建已经挡掉了 `.env` 的内容；如果读取工具放行，模型只要主动调
    /// 一次就能拿到同样的东西，出网过滤等于白做。
    #[test]
    fn credential_files_are_unreadable_and_unsearchable() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write(".env", "STRIPE_SECRET_KEY=sk_live_deadbeef\n");
        env.write("src/app.ts", "const token = readEnv();\n");

        let err = read_file_tool(".env").unwrap_err();
        assert!(err.contains("credential"), "{}", err);

        // 搜不到：`.env` 不参与遍历。注意"无匹配"的回执里会回显查询串本身，
        // 所以这里断言的是没有命中该文件，而不是回执里不含这段文本。
        let results = search_text_tool("STRIPE_SECRET_KEY", None).unwrap();
        assert!(!results.contains(".env"), "{}", results);
        assert!(results.starts_with("No matches"), "{}", results);

        // 普通文件照常可搜
        let results = search_text_tool("readEnv", None).unwrap();
        assert!(results.contains("src/app.ts:1"), "{}", results);
    }

    #[test]
    fn search_filters_by_extension_and_skips_dependency_dirs() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "findMe();\n");
        env.write("src/app.py", "findMe()\n");
        env.write("node_modules/pkg/index.ts", "findMe();\n");

        let all = search_text_tool("findMe", None).unwrap();
        assert!(all.contains("src/app.ts"));
        assert!(all.contains("src/app.py"));
        // 依赖树里的命中是噪音，会把上下文预算吃光
        assert!(!all.contains("node_modules"), "{}", all);

        let only_ts = search_text_tool("findMe", Some("ts")).unwrap();
        assert!(only_ts.contains("src/app.ts"));
        assert!(!only_ts.contains("app.py"), "{}", only_ts);
    }

    #[test]
    fn list_files_hides_dependency_dirs() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "");
        env.write("node_modules/pkg/index.js", "");
        env.write("README.md", "");

        let listing = list_files_tool(".").unwrap();

        assert!(listing.contains("src/"));
        assert!(listing.contains("README.md"));
        assert!(!listing.contains("node_modules"), "{}", listing);
    }

    #[test]
    fn tool_names_do_not_collide_with_mcp_routing() {
        let permissions = WorkspaceToolPermissions::with_commands(vec!["npm test".to_string()]);
        let invoker = WorkspaceToolInvoker::without_logging(permissions.clone());
        for definition in tool_definitions(&permissions) {
            assert!(definition.name.starts_with(WORKSPACE_TOOL_PREFIX));
            assert!(invoker.handles(&definition.name));
            assert!(!crate::services::mcp::is_mcp_tool_name(&definition.name));
        }
        assert!(!invoker.handles("mcp__files__read"));
    }

    /// 未授权时命令工具不该出现在工具列表里，也不该被认领。
    ///
    /// 通告一个必然失败的工具比不通告更糟：模型会去调它，浪费一轮，然后才
    /// 从错误里学到它用不了。
    #[test]
    fn command_tool_is_absent_without_permission() {
        let read_only = WorkspaceToolPermissions::read_only();
        let names: Vec<String> = tool_definitions(&read_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();

        assert!(!names.contains(&RUN_COMMAND.to_string()), "{:?}", names);
        assert!(!WorkspaceToolInvoker::without_logging(read_only).handles(RUN_COMMAND));

        let with_commands = WorkspaceToolPermissions::with_commands(vec!["cargo test".to_string()]);
        let names: Vec<String> = tool_definitions(&with_commands)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(names.contains(&RUN_COMMAND.to_string()));
        assert!(WorkspaceToolInvoker::without_logging(with_commands).handles(RUN_COMMAND));
    }

    /// 允许清单之外的命令必须拒绝，长驻命令即使在清单里也必须拒绝。
    ///
    /// 后者是安全不变量而不是偏好：验证是跑完再看结果，`npm run dev` 不会退出，
    /// 放行它等于挂住整个 stage 直到取消。
    #[tokio::test]
    async fn command_tool_enforces_allow_list_and_refuses_long_running() {
        let allowed = vec!["npm test".to_string(), "cargo *".to_string()];

        let error = run_command_tool("rm -rf /", &allowed, test_cancel())
            .await
            .unwrap_err();
        assert!(error.contains("not authorized"), "{}", error);

        // 前缀通配命中，但这是长驻命令 —— 先判长驻，所以给出的是长驻的理由
        let error = run_command_tool("cargo watch -x test", &allowed, test_cancel())
            .await
            .unwrap_err();
        assert!(error.contains("long-running"), "{}", error);

        // 清单里写了也不行：长驻判定不受清单覆盖
        let error = run_command_tool("npm run dev", &["npm run dev".to_string()], test_cancel())
            .await
            .unwrap_err();
        assert!(error.contains("long-running"), "{}", error);
    }

    /// 命令跑完之后，退出码、耗时和输出都要回给模型 —— 这是"闭环"的关键：
    /// 没有真实输出，模型只能猜自己的改动有没有生效。
    ///
    /// 用 `block_on` 而不是 `#[tokio::test]`：`env_test_guard` 是同步锁，
    /// 在 async 测试里跨 await 持有它会触发 `await_holding_lock`，而这个守卫
    /// 保护的正是被调用方要读的 `AGENT_IDE_CONFIG_DIR`，不能提前放掉。
    #[test]
    fn command_tool_reports_exit_code_and_output() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "");

        let command = if cfg!(windows) {
            "cmd /C exit 3"
        } else {
            "sh -c 'exit 3'"
        };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_command_tool(command, &[command.to_string()], test_cancel()))
            .unwrap();

        assert!(output.contains("exit code: 3"), "{}", output);
        assert!(output.contains("duration:"), "{}", output);
    }

    /// 未授权时写入工具不该出现，也不该被认领 —— 和命令工具同一条规则。
    #[test]
    fn write_tool_is_absent_without_permission() {
        let read_only = WorkspaceToolPermissions::read_only();
        let names: Vec<String> = tool_definitions(&read_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(!names.contains(&WRITE_FILE.to_string()), "{:?}", names);
        assert!(!WorkspaceToolInvoker::without_logging(read_only).handles(WRITE_FILE));

        let writable = WorkspaceToolPermissions::new(Vec::new(), true, true);
        let names: Vec<String> = tool_definitions(&writable)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(names.contains(&WRITE_FILE.to_string()));
        assert!(WorkspaceToolInvoker::without_logging(writable).handles(WRITE_FILE));
    }

    /// 写入必须记下写之前的内容：撤销靠它，事后合成的 diff 卡片也靠它才能
    /// 显示"改了什么"。
    #[test]
    fn write_tool_records_previous_content_for_undo() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "const value = 1;\n");
        let permissions = WorkspaceToolPermissions::new(Vec::new(), true, true);

        let created = write_file_tool("src/new.ts", "export const a = 1;\n", &permissions).unwrap();
        assert!(created.contains("Created"), "{}", created);

        let updated = write_file_tool("src/app.ts", "const value = 2;\n", &permissions).unwrap();
        assert!(updated.contains("Updated"), "{}", updated);

        let writes = permissions.take_writes();
        assert_eq!(writes.len(), 2);
        assert!(writes[0].previous.is_none());
        assert_eq!(writes[1].previous.as_deref(), Some("const value = 1;\n"));
        assert_eq!(writes[1].updated, "const value = 2;\n");
        // 取过之后就清空，避免同一批写入被登记两次
        assert!(permissions.take_writes().is_empty());

        // 内容没变时不记录：给撤销栈塞一个什么都没改的回滚点会让栈顶失真
        let unchanged = write_file_tool("src/app.ts", "const value = 2;\n", &permissions).unwrap();
        assert!(unchanged.contains("nothing written"), "{}", unchanged);
        assert!(permissions.take_writes().is_empty());
    }

    /// 凭据文件和 `.git/` 由 `resolve_for_agent_write` 拒绝，新建文件另需授权。
    ///
    /// 断言的重点是写入工具走的是和 diff 应用同一套拒绝清单，而不是自己另写
    /// 一份判断 —— 两份判断迟早会分叉。
    #[test]
    fn write_tool_refuses_denied_paths_and_unauthorized_creates() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "const value = 1;\n");

        let permissions = WorkspaceToolPermissions::new(Vec::new(), true, true);
        let error = write_file_tool(".env", "SECRET=1\n", &permissions).unwrap_err();
        assert!(error.to_lowercase().contains("credential"), "{}", error);
        let error =
            write_file_tool(".git/hooks/pre-commit", "#!/bin/sh\n", &permissions).unwrap_err();
        assert!(!error.is_empty());

        // allow_write 但不允许新建：只能改已存在的文件
        let edit_only = WorkspaceToolPermissions::new(Vec::new(), true, false);
        let error = write_file_tool("src/brand-new.ts", "x\n", &edit_only).unwrap_err();
        assert!(
            error.contains("creating files is not authorized"),
            "{}",
            error
        );
        assert!(write_file_tool("src/app.ts", "const value = 3;\n", &edit_only).is_ok());

        // 完全没有写权限时，即使调到了也必须拒绝，而不是只依赖"没通告出去"
        let read_only = WorkspaceToolPermissions::read_only();
        let error = write_file_tool("src/app.ts", "x\n", &read_only).unwrap_err();
        assert!(error.contains("not authorized"), "{}", error);
    }

    /// 移动记的是"从哪来"，不是"删一条 + 建一条"。
    ///
    /// 两条记录会在审查区变成两张互不相干的卡片，各自的 rationale 还会指向没被调用的
    /// 工具；这里断言的是一条记录带着 `moved_from`，以及磁盘上真的搬过去了。
    #[test]
    fn move_tool_records_where_the_file_came_from() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/old.ts", "export const a = 1;\n");
        let permissions = WorkspaceToolPermissions::new(Vec::new(), true, true);

        let message = move_file_tool("src/old.ts", "src/nested/new.ts", &permissions).unwrap();
        assert!(message.contains("src/old.ts"), "{}", message);

        assert!(!env.root.join("src/old.ts").exists());
        assert_eq!(
            std::fs::read_to_string(env.root.join("src/nested/new.ts")).unwrap(),
            "export const a = 1;\n"
        );

        let writes = permissions.take_writes();
        assert_eq!(writes.len(), 1, "一次移动是一条记录");
        assert_eq!(writes[0].file, "src/nested/new.ts");
        let source = writes[0].moved_from.as_ref().expect("moved_from");
        assert_eq!(source.file, "src/old.ts");
        // 撤销靠移回去，所以不需要内容；`previous` 是 None 正是这个意思
        assert!(writes[0].previous.is_none());
    }

    /// 目标已存在时必须拒绝：覆盖会毁掉目标原有的内容，而那份内容没有任何记录能撤销。
    #[test]
    fn move_tool_refuses_to_overwrite_and_to_move_denied_paths() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/a.ts", "a\n");
        env.write("src/b.ts", "b\n");
        env.write(".env", "SECRET=1\n");
        let permissions = WorkspaceToolPermissions::new(Vec::new(), true, true);

        let error = move_file_tool("src/a.ts", "src/b.ts", &permissions).unwrap_err();
        assert!(error.contains("already exists"), "{}", error);
        // 被拒绝的调用不能留下任何记录，否则审查区会出现一次没发生的移动
        assert!(permissions.take_writes().is_empty());
        assert_eq!(std::fs::read_to_string(env.root.join("src/b.ts")).unwrap(), "b\n");

        // 拒绝清单两端都管：凭据文件既不能当源也不能当目标
        let error = move_file_tool(".env", "src/leaked.ts", &permissions).unwrap_err();
        assert!(error.to_lowercase().contains("credential"), "{}", error);
        let error = move_file_tool("src/a.ts", ".env.local", &permissions).unwrap_err();
        assert!(error.to_lowercase().contains("credential"), "{}", error);

        let error = move_file_tool("src/missing.ts", "src/c.ts", &permissions).unwrap_err();
        assert!(error.contains("does not exist"), "{}", error);

        // 目录不接受，和 delete 一致
        let error = move_file_tool("src", "src2", &permissions).unwrap_err();
        assert!(error.contains("directory"), "{}", error);
    }

    /// Stop 之后，已经排到工具里的调用也不许再产生副作用。
    ///
    /// 取消原本只在两次工具调用**之间**生效：一次已经进到 `invoke` 的调用照旧会跑命令、
    /// 照旧会打开页面 —— 界面显示空闲，而世界还在被改。撤不回的动作尤其不能这样。
    #[test]
    fn stop_refuses_side_effecting_tools_and_records_the_browser_attempt() {
        let mut permissions = WorkspaceToolPermissions::new(Vec::new(), true, true)
            .with_browser(true, vec!["*".to_string()]);
        let switch = test_cancel();
        permissions.adopt_cancel(switch.clone());
        switch.store(true, std::sync::atomic::Ordering::Relaxed);
        let invoker = WorkspaceToolInvoker::without_logging(permissions.clone());
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let error = runtime
            .block_on(invoker.invoke(WRITE_FILE, "{\"path\":\"a.ts\",\"content\":\"x\"}"))
            .unwrap_err();
        assert!(error.contains("stopped"), "{}", error);
        // 拒绝掉的写入不能留下审查区卡片：那份卡片会声称磁盘上有一次没发生的改动
        assert!(permissions.take_writes().is_empty());

        let error = runtime
            .block_on(invoker.invoke(BROWSER_OPEN, "{\"url\":\"https://example.com/\"}"))
            .unwrap_err();
        assert!(error.contains("stopped"), "{}", error);
        // 没发生，但"停了之后模型还想出网"要留痕
        let actions = permissions.take_external_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "browser_open_cancelled");
        assert_eq!(actions[0].target, "https://example.com/");

        // 桌面观察也要留痕。上一版只记了浏览器：加了新工具没检查记录侧，那条尝试
        // 就只剩一行普通日志，而外部动作列表 —— 用户被告知要看的那个地方 —— 空着。
        let mut desktop = WorkspaceToolPermissions::new(Vec::new(), false, false)
            .with_computer(true, vec!["*".to_string()]);
        desktop.adopt_cancel(switch.clone());
        let desktop_invoker = WorkspaceToolInvoker::without_logging(desktop.clone());
        let error = runtime
            .block_on(desktop_invoker.invoke(COMPUTER_WINDOWS, "{}"))
            .unwrap_err();
        assert!(error.contains("stopped"), "{}", error);
        let actions = desktop.take_external_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "computer_windows_cancelled");
        assert_eq!(actions[0].target, "desktop");
    }

    /// 只读工具在 Stop 之后照旧放行：它们不改变任何东西，拒掉只会给一份马上要丢掉的
    /// 对话再添一条噪声。
    #[test]
    fn stop_does_not_block_read_only_tools() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("readme.md", "hello\n");
        let mut permissions = WorkspaceToolPermissions::read_only();
        let switch = test_cancel();
        permissions.adopt_cancel(switch.clone());
        switch.store(true, std::sync::atomic::Ordering::Relaxed);
        let invoker = WorkspaceToolInvoker::without_logging(permissions);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let output = runtime
            .block_on(invoker.invoke(READ_FILE, "{\"path\":\"readme.md\"}"))
            .expect("read-only tools stay available");
        assert!(output.contains("hello"), "{}", output);
    }

    /// 浏览器工具需要**两样**：开关，以及一份非空的 origin 清单。
    ///
    /// 空清单不当成"没配置就全放"：默认放开的清单在出事那天读起来像是用户批准过。
    #[test]
    fn browser_tools_need_the_switch_and_a_non_empty_allowlist() {
        let switch_only = WorkspaceToolPermissions::new(Vec::new(), false, false)
            .with_browser(true, Vec::new());
        let names: Vec<String> = tool_definitions(&switch_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(!names.contains(&BROWSER_OPEN.to_string()), "{:?}", names);
        assert!(!WorkspaceToolInvoker::without_logging(switch_only.clone()).handles(BROWSER_OPEN));
        // 即使被直接调用也要拒绝，不能只靠"没通告出去"
        assert!(browser_open_tool("https://example.com/", &switch_only).is_err());
        assert!(browser_tabs_tool(&switch_only).is_err());
        // 权限关着时的调用也要留痕：它和"站点不在清单里"是同一类信息 —— 模型想出网
        let refusals = switch_only.take_external_actions();
        assert_eq!(
            refusals
                .iter()
                .map(|action| action.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["browser_open_refused", "browser_tabs_refused"]
        );


        let granted = WorkspaceToolPermissions::new(Vec::new(), false, false)
            .with_browser(true, vec!["https://example.com".to_string()]);
        let names: Vec<String> = tool_definitions(&granted)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(names.contains(&BROWSER_OPEN.to_string()));
        assert!(names.contains(&BROWSER_TABS.to_string()));
        assert!(WorkspaceToolInvoker::without_logging(granted).handles(BROWSER_TABS));
    }

    /// 读图片的工具把图片挂到这次调用上，执行器再取走。
    ///
    /// 断言落在"取一次就没了"上：不排空的话，这次的图会跟到下一个工具结果上，模型看到的
    /// 图和它问的问题就错位了 —— 那种错比没有图更难查。
    #[test]
    fn reading_an_image_attaches_it_once() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        // 一个最小的 PNG 头就够了：这一层不解码，只判扩展名和大小
        std::fs::write(env.root.join("mock.png"), [0x89, 0x50, 0x4E, 0x47]).unwrap();
        let permissions = WorkspaceToolPermissions::read_only();

        let text = read_image_tool("mock.png", &permissions).expect("image is attached");
        assert!(text.contains("image/png"), "{}", text);
        assert!(text.contains("mock.png"), "{}", text);

        let images = permissions.take_images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].media_type, "image/png");
        assert!(images[0].data_url().starts_with("data:image/png;base64,"));
        // 取过一次就空了
        assert!(permissions.take_images().is_empty());
        // 预算按整次运行累计，所以排空图片不能把账也清掉
        assert_eq!(permissions.images_bytes_used(), 4);
    }

    /// 一次运行的图片总量要有账，而被拒的读取不能占账。
    ///
    /// 单张 4 MiB 的上限管不住重复调用：模型可以一轮一张地读下去，每张都合规。这里
    /// 断言的是计数器随成功的读取累加、且解析失败（不是图片）不计 —— 不然一串
    /// `.txt` 就能把合法图片的预算耗光。
    #[test]
    fn the_run_image_budget_counts_only_what_was_attached() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        std::fs::write(env.root.join("a.png"), [0x89, 0x50, 0x4E, 0x47]).unwrap();
        std::fs::write(env.root.join("b.png"), [0x89, 0x50, 0x4E, 0x47, 0x0D]).unwrap();
        env.write("notes.txt", "hello");
        let permissions = WorkspaceToolPermissions::read_only();

        read_image_tool("a.png", &permissions).expect("first image is attached");
        assert_eq!(permissions.images_bytes_used(), 4);
        read_image_tool("b.png", &permissions).expect("second image is attached");
        assert_eq!(permissions.images_bytes_used(), 9);

        read_image_tool("notes.txt", &permissions).unwrap_err();
        assert_eq!(
            permissions.images_bytes_used(),
            9,
            "a refused read must not spend the run's budget"
        );
    }


    /// 不是图片的文件要在这里就被拒，而不是发出去让 provider 报一个看不懂的错。
    #[test]
    fn reading_a_non_image_is_refused_locally() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("notes.txt", "hello");
        let permissions = WorkspaceToolPermissions::read_only();

        let error = read_image_tool("notes.txt", &permissions).unwrap_err();
        assert!(error.contains("png, jpg, gif, webp"), "{}", error);
        assert!(permissions.take_images().is_empty());
    }

    /// 桌面观察和浏览器同构：开关 + 非空应用清单，缺一不可，被拒也要留痕。
    ///
    /// 这个工具只读，但它披露窗口标题 —— 文档名、网页标题、聊天对象都在里面，所以
    /// 授权和记录按同一套来。
    #[test]
    fn desktop_observation_needs_the_switch_and_a_non_empty_app_list() {
        let switch_only =
            WorkspaceToolPermissions::new(Vec::new(), false, false).with_computer(true, Vec::new());
        let names: Vec<String> = tool_definitions(&switch_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(!names.contains(&COMPUTER_WINDOWS.to_string()), "{:?}", names);
        assert!(
            !WorkspaceToolInvoker::without_logging(switch_only.clone()).handles(COMPUTER_WINDOWS)
        );
        assert!(computer_windows_tool(&switch_only).is_err());
        // 这条记录是给直接调用兜底用的：真实运行里 `handles()` 会先把调用丢掉，
        // 所以 `computer_windows_refused` 不会出现在一次真的运行的记录里
        let refusals = switch_only.take_external_actions();
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].kind, "computer_windows_refused");

        // 通告里要写出允许的应用，否则模型看不到范围
        let granted = WorkspaceToolPermissions::new(Vec::new(), false, false)
            .with_computer(true, vec!["Code.exe".to_string()]);
        let advertised = tool_definitions(&granted)
            .into_iter()
            .find(|definition| definition.name == COMPUTER_WINDOWS);
        if cfg!(windows) {
            let description = advertised.expect("tool is advertised on Windows").description;
            assert!(description.contains("Code.exe"), "{}", description);
            // 也要说清这是子集，否则模型会把过滤后的列表当成整个桌面
            assert!(description.contains("hidden"), "{}", description);
        } else {
            // 没有实现的平台上不通告：一个必然失败的工具会被模型反复调用，
            // 而它的失败看起来像"桌面上没有窗口"
            assert!(advertised.is_none());
        }
    }


    /// 通告里要写出授权的站点：模型看不到范围时只会不断试探被拒的站点。
    #[test]
    fn the_allowlist_is_visible_in_the_tool_description() {
        let granted = WorkspaceToolPermissions::default()
            .with_browser(true, vec!["http://127.0.0.1:1420".to_string()]);

        let description = tool_definitions(&granted)
            .into_iter()
            .find(|definition| definition.name == BROWSER_OPEN)
            .expect("browser tool")
            .description;

        assert!(description.contains("http://127.0.0.1:1420"), "{}", description);
        // 也要说清它撤不回，否则模型会以为这和写文件一样可以回滚
        assert!(description.to_lowercase().contains("cannot be undone"));
    }

    /// 被拒绝的调用也要留痕：'模型试图打开一个没授权的站点'正是用户事后最想知道的事。
    #[test]
    fn a_refused_origin_is_recorded_not_just_returned() {
        let granted = WorkspaceToolPermissions::default()
            .with_browser(true, vec!["http://127.0.0.1:1420".to_string()]);

        let error = browser_open_tool("https://evil.example/steal", &granted).unwrap_err();
        assert!(error.contains("not in the allowed origins"), "{}", error);

        let actions = granted.take_external_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "browser_open_refused");
        assert_eq!(actions[0].target, "https://evil.example");
    }

    /// scheme 不对时同样记录，而且在任何网络请求之前就拒掉。
    #[test]
    fn a_hostile_scheme_never_reaches_the_browser() {
        let granted = WorkspaceToolPermissions::default().with_browser(true, vec!["*".to_string()]);

        for hostile in [
            "javascript:alert(document.cookie)",
            "file:///c:/Windows/System32/drivers/etc/hosts",
            "chrome://settings",
        ] {
            assert!(browser_open_tool(hostile, &granted).is_err(), "{}", hostile);
        }

        let actions = granted.take_external_actions();
        assert_eq!(actions.len(), 3);
        assert!(actions.iter().all(|action| action.kind == "browser_open_refused"));
    }

    /// 两个授权位都要有，而且缺哪个都不通告、也不认领。
    #[test]
    fn move_tool_needs_both_write_and_create() {
        let edit_only = WorkspaceToolPermissions::new(Vec::new(), true, false);
        let names: Vec<String> = tool_definitions(&edit_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(!names.contains(&MOVE_FILE.to_string()), "{:?}", names);
        assert!(!WorkspaceToolInvoker::without_logging(edit_only.clone()).handles(MOVE_FILE));

        let full = WorkspaceToolPermissions::new(Vec::new(), true, true);
        let names: Vec<String> = tool_definitions(&full)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(names.contains(&MOVE_FILE.to_string()));
        assert!(WorkspaceToolInvoker::without_logging(full).handles(MOVE_FILE));

        // 即使被直接调用也要拒绝，不能只依赖"没通告出去"
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/a.ts", "a\n");
        let error = move_file_tool("src/a.ts", "src/b.ts", &edit_only).unwrap_err();
        assert!(error.contains("not authorized"), "{}", error);
        let read_only = WorkspaceToolPermissions::read_only();
        let error = move_file_tool("src/a.ts", "src/b.ts", &read_only).unwrap_err();
        assert!(error.contains("not authorized"), "{}", error);
        assert!(env.root.join("src/a.ts").exists());
    }
}
