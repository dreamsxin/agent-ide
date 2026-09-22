/** Agent 状态枚举 */
export type AgentState =
  | "idle"
  | "thinking"
  | "planning"
  | "acting"
  | "reviewing"
  | "waiting_user"
  | "done"
  | "error";

/**
 * Agent 控制模式：改动是等人审查，还是跑完直接落盘。
 *
 * 只有两档。后端所有权限门都只问"是不是 auto"，所以曾经的第三档 `edit` 和
 * `suggest` 逐位相同 —— 一个拨了不会有任何区别的位置。更细的授权在
 * SettingsPanel 的权限开关里（能不能新建文件、能不能跑命令）。
 */
export type AgentMode = "suggest" | "auto";

/**
 * 把外部来源的模式字符串收敛成合法值。
 *
 * 需要它的地方有两处，都是不受本进程控制的输入：localStorage 里上个版本存下的
 * 会话（可能是已删掉的 `"edit"`），以及后端事件里的模式字段。两处原本都是
 * `as AgentMode` 硬转，那不是校验，只是让类型检查闭嘴 —— 真值仍然会漏进 store，
 * 让分段控件渲染出一个哪一段都没选中的状态。
 */
export function normalizeAgentMode(value: unknown): AgentMode {
  return value === "auto" ? "auto" : "suggest";
}


/** IDE 工作模式，独立于 Agent 权限模式 */
export type IdeMode = "code" | "plan";

/**
 * 一次运行至今的用量，随 `agent-state-changed` 一起送来。
 *
 * `spendMicros` 为 null 是"没配价格，算不出来"，**不是**"没花钱"。
 * `calls` / `reportedCalls` 分开是因为供应商可能不回报用量：那时 token 数是 0，
 * 但那代表"不知道"。
 */
export interface RunUsage {
  totalTokens: number;
  maxTotalTokens: number | null;
  spendMicros: number | null;
  maxSpendMicros: number | null;
  calls: number;
  reportedCalls: number;
}

/** 事件里的 usage 字段收敛成 `RunUsage`，非法或缺失时返回 null */
/**
 * 上下文占用测量值的归一化。
 *
 * 和 `normalizeRunUsage` 同一个理由：事件载荷不可信。多一条规则 —— 总数为 0 时返回
 * null，因为 0 在这里不是"空的上下文"，而是"这次运行没有一个请求回报过用量"，
 * 界面必须什么都不显示而不是显示 0%。
 */
export function normalizeContextUsage(value: unknown): ContextUsageMeasurement | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const count = (key: string): number => {
    const candidate = raw[key];
    return typeof candidate === "number" && Number.isFinite(candidate) && candidate > 0
      ? Math.floor(candidate)
      : 0;
  };
  const lastTotalTokens = count("lastTotalTokens");
  if (lastTotalTokens === 0) return null;
  return { lastPromptTokens: count("lastPromptTokens"), lastTotalTokens };
}

export function normalizeRunUsage(value: unknown): RunUsage | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  // `typeof NaN === "number"`，所以光看 typeof 不够：NaN 或负数漏进去，
  // 状态栏就会渲染出 `$NaN.0NaN` 这种东西。计数不可能为负，钳到 0。
  const finite = (key: string): number | null => {
    const candidate = raw[key];
    return typeof candidate === "number" && Number.isFinite(candidate) ? candidate : null;
  };
  const count = (key: string) => Math.max(0, finite(key) ?? 0);
  const optional = (key: string) => {
    const candidate = finite(key);
    return candidate === null ? null : Math.max(0, candidate);
  };
  return {
    totalTokens: count("totalTokens"),
    maxTotalTokens: optional("maxTotalTokens"),
    spendMicros: optional("spendMicros"),
    maxSpendMicros: optional("maxSpendMicros"),
    calls: count("calls"),
    reportedCalls: count("reportedCalls"),
  };
}

/**
 * 把用量说成状态栏那一格能放下的一句话，以及悬停时的完整明细。
 *
 * 返回 null 表示这次会话还没发出过任何请求 —— 那时整格不渲染，而不是显示 0。
 *
 * 三档必须分开，理由和后端 `action_log_summary` 一样：完全没回报时说 "unknown"
 * 而不是打印 0（后者让人以为免费）；部分回报比完全不回报更危险，总数看起来正常
 * 但漏掉的调用不进账、per-run 上限因此偏松，所以明说。
 */
export function describeRunUsage(
  usage: RunUsage,
  formatSpend: (micros: number) => string
): { label: string; detail: string } | null {
  if (usage.calls === 0) return null;

  const parts = [`${usage.calls} call${usage.calls === 1 ? "" : "s"}`];
  if (usage.maxTotalTokens !== null) {
    parts.push(`token cap ${usage.maxTotalTokens}`);
  }
  if (usage.maxSpendMicros !== null) {
    parts.push(`spend cap ${formatSpend(usage.maxSpendMicros)}`);
  }

  if (usage.reportedCalls === 0) {
    return {
      label: "usage unknown",
      detail: `The provider reported no token usage, so this run's cost cannot be estimated (${parts.join(", ")}).`,
    };
  }

  const spend = usage.spendMicros === null ? null : formatSpend(usage.spendMicros);
  // 部分回报时用 `≥` 把"这是下界"摆在**可见**的标签上，而不是只写进 tooltip：
  // 只有 hover 才看得到的限定词，键盘和读屏用户永远看不到，而那正是这条信息
  // 最要紧的部分 —— 数字看起来正常，实际偏低，per-run 上限也因此偏松。
  const bound = usage.reportedCalls < usage.calls ? "\u2265" : "";
  const label = spend
    ? `${bound}${usage.totalTokens} tok · ${bound}${spend}`
    : `${bound}${usage.totalTokens} tok`;
  const partial =
    usage.reportedCalls < usage.calls
      ? ` Only ${usage.reportedCalls} of ${usage.calls} calls reported usage, so this is a lower bound and the per-run cap undercounts.`
      : "";
  const cost = spend ? `, estimated ${spend}` : ", cost not computable (no pricing configured)";
  return {
    label,
    detail: `${usage.totalTokens} tokens${cost} (${parts.join(", ")}).${partial}`,
  };
}


/**
 * 权限预设：一个梯子，每一档在前一档之上多放开一件事。
 *
 * 值名刻意不叫 `ask` / `suggest` / `auto` —— 后两个和 `AgentMode` 的取值撞名却
 * 含义不同，选 `suggest` 预设并不会让运行进入 `suggest` 模式。既然两个轴必须
 * 并存，那就让名字自己说清它授予什么，而不是靠文档去解释一个陷阱。
 */
export type AgentPermissionPreset = "read-only" | "create-files" | "run-commands";


/**
 * 一次运行授予 Agent 的细粒度权限。
 *
 * 只有两项，因为只有两项真的过 IPC 并在后端被检查。曾经还有
 * `allowFileDelete` 和 `allowGitActions`：两个界面上拨得动、后端没有任何读者的
 * 开关。它们比没有更糟 —— 关掉"文件删除"会让人以为堵上了一条路，而那条路
 * 从来不存在；打开"Git 操作"会让人以为开了一条路，而 Agent 根本不跑 Git。
 * 等真的出现对应的后端路径时再把开关加回来，那时它才有东西可守。
 */
export interface AgentPermission {
  allowFileCreate: boolean;
  allowCommandRun: boolean;
  /**
   * 是否允许 Agent 驱动浏览器。
   *
   * 和写盘分开：打开一个页面不改工作区，但它会把工作区里的内容送到某个站点去，而且
   * 导航**撤不回**。后端两道闸门都要过（这个开关 + 下面的 origin 清单），所以单独打开
   * 它不会让 Agent 能去任何地方。
   */
  allowBrowserUse: boolean;
  /**
   * 允许访问的 origin（`scheme://host[:port]`，`*` 表示不限）。
   *
   * 空清单等于不许，即使开关是开的 —— 默认放开的清单在出事那天读起来像是用户批准过。
   */
  browserOrigins: string[];
  /**
   * 是否允许 Agent 读取一个已经打开的页面的正文。
   *
   * 和 `allowBrowserUse` 分开，因为披露的东西不是一个量级：标签页列表说的是"你开着
   * 这个站点"，正文说的是站点上的**内容** —— 包括只有登录之后才看得到的那部分。
   * 后端也是两道闸门：这个开关 + 下面的 origin 清单。它不点击、不输入、不导航。
   */
  allowPageRead: boolean;
  /**
   * 允许被读取正文的 origin（`scheme://host[:port]`，`*` 表示不限）。
   *
   * 独立于 `browserOrigins`：允许把一份文档**打开**，不等于允许把它的正文抄给模型。
   */
  pageReadOrigins: string[];
  /**
   * 是否允许 Agent 观察桌面（枚举可见窗口，只读）。
   *
   * 和浏览器分开：那边的范围是"哪些站点"，这里是"哪些应用"，互不蕴含。窗口标题里有
   * 文档名、网页标题、聊天对象，所以后端同样要两道闸门：这个开关 + 下面的应用清单。
   * 目前只有 Windows 有实现，其他平台上工具不会通告。
   */
  allowComputerUse: boolean;
  /**
   * 允许被观察的应用（可执行文件名，`*` 表示不限）。
   *
   * 空清单等于不许。它过滤的是**结果**：不在清单里的窗口不会出现在返回值里，只回报
   * 被挡掉的数量。
   */
  computerApps: string[];
  /**
   * 是否允许 Agent 截取窗口内容。
   *
   * 和 `allowComputerUse` 分开，因为披露的东西不是一个量级：窗口标题说"Signal 开着"，
   * 一张截图把里面的消息都交出去了。共用一个开关等于替用户把他给过的"能看列表"
   * 悄悄升级成"能看内容"。同样只有 Windows 有实现。
   */
  allowComputerCapture: boolean;
  /** 允许被截图的应用（进程名，`*` 表示不限）。 */
  captureApps: string[];
  /**
   * 是否允许 Agent 往一个窗口里注入点击。
   *
   * 这个产品里最狠的一档：一次点击撤不回，而且它能点掉任何一个确认框 —— 包括本产品
   * 自己那个。后端除了这个开关和下面的清单，还要求那一下必须对着模型**已经截过的那一
   * 帧**给坐标，并且每次都要人工批准。
   */
  allowComputerInput: boolean;
  /** 允许被点击的应用（进程名，`*` 表示不限）。独立于 `captureApps`。 */
  inputApps: string[];
}

/**
 * 只有布尔那几项能被"切换"。
 *
 * `browserOrigins` 是一个数组，落进 `togglePermission` 会被 `!value` 变成 `false`，
 * 一个类型上不该存在的值就这样进了 store。用类型把它挡在外面，而不是靠调用方记得。
 */
export type BooleanPermissionKey = {
  [K in keyof AgentPermission]: AgentPermission[K] extends boolean ? K : never;
}[keyof AgentPermission];

/** `read-only` 预设：既不新建文件，也不跑命令，也不碰浏览器。 */
export const READ_ONLY_PERMISSIONS: AgentPermission = {
  allowFileCreate: false,
  allowCommandRun: false,
  allowBrowserUse: false,
  browserOrigins: [],
  allowPageRead: false,
  pageReadOrigins: [],
  allowComputerUse: false,
  computerApps: [],
  allowComputerCapture: false,
  captureApps: [],
  allowComputerInput: false,
  inputApps: [],
};

/** `create-files` 预设：可以新建文件（改动仍进审查区），但不跑命令。 */
export const CREATE_FILES_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowCommandRun: false,
  allowBrowserUse: false,
  browserOrigins: [],
  allowPageRead: false,
  pageReadOrigins: [],
  allowComputerUse: false,
  computerApps: [],
  allowComputerCapture: false,
  captureApps: [],
  allowComputerInput: false,
  inputApps: [],
};

/**
 * `run-commands` 预设：可以新建文件，也可以跑项目自己声明的命令。
 *
 * 浏览器不跟着这个预设一起开：能跑本地检查命令和能访问外部站点是两件不同性质的事，
 * 把它们绑在一个预设里，用户为了跑测试就顺手批准了出网。
 */
export const RUN_COMMANDS_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowCommandRun: true,
  allowBrowserUse: false,
  browserOrigins: [],
  allowPageRead: false,
  pageReadOrigins: [],
  allowComputerUse: false,
  computerApps: [],
  allowComputerCapture: false,
  captureApps: [],
  allowComputerInput: false,
  inputApps: [],
};

/** 根据预设获取权限 */
export function permissionsForPreset(preset: AgentPermissionPreset): AgentPermission {
  switch (preset) {
    case "read-only": return { ...READ_ONLY_PERMISSIONS };
    case "create-files": return { ...CREATE_FILES_PERMISSIONS };
    case "run-commands": return { ...RUN_COMMANDS_PERMISSIONS };
  }
}

/** MCP 工具放行策略，与后端 `McpToolPolicy` 一一对应 */
export type McpToolApproval = "deny" | "auto_approved_only" | "allow_all";

/**
 * 由 Agent 权限推导 MCP 工具放行策略。
 *
 * MCP 工具本质上是外部进程执行（可读写文件、访问网络、跑命令），
 * 因此跟随 `allowCommandRun`：未授予命令执行权限时，只允许用户在
 * server 配置里显式列入 autoApprove 的工具。
 */
export function mcpApprovalForPermissions(permissions: AgentPermission): McpToolApproval {
  return permissions.allowCommandRun ? "allow_all" : "auto_approved_only";
}

/**
 * 需要人点一下才能发生的操作类型。
 *
 * 只列后端真的会发的那些。之前这里还有 `file_delete` / `command_run` / `git_push` /
 * `git_force` 四种，没有任何后端路径产生它们 —— 连同对话框里对应的图标和标签，
 * 那是四份看起来像"这些操作有确认"的假象。
 */
export type DestructiveOpType =
  | "browser_open"
  | "browser_read_page"
  | "computer_capture"
  | "computer_click"
  | "computer_scroll";

export interface DestructiveOpConfirm {

  id: string;
  opType: DestructiveOpType;
  title: string;
  description: string;
  detail: string;
}

/**
 * 把后端事件里的 `opType` 收敛成已知取值。
 *
 * 不用 `as DestructiveOpType`：那只是让 tsc 闭嘴。后端加了一种新动作而前端还没认识
 * 它时，硬转会让界面按一个不存在的分支渲染；落到某个具体已知值上更糟 —— 对话框会说错
 * 将要发生什么，而这个对话框的全部意义就是说对。所以未知值单独一档。
 */
export function normalizeDestructiveOpType(value: unknown): DestructiveOpType | "unknown" {
  switch (value) {
    case "browser_open":
    case "browser_read_page":
    case "computer_capture":
    case "computer_click":
    case "computer_scroll":
      return value;
    default:

      return "unknown";
  }
}

/**
 * 把后端 `agent-approval-requested` 的载荷收成一条待批准记录。
 *
 * 缺字段就返回 null 而不是补默认值：一条说不清"将要发生什么"的批准请求不该被显示，
 * 显示它等于请用户为一件他看不见的事签字。
 */
export function normalizeApprovalRequest(value: unknown): DestructiveOpConfirm | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  const raw = value as Record<string, unknown>;
  const id = typeof raw.id === "string" ? raw.id : "";
  const title = typeof raw.title === "string" ? raw.title : "";
  const description = typeof raw.description === "string" ? raw.description : "";
  if (!id || !title || !description) {
    return null;
  }
  const opType = normalizeDestructiveOpType(raw.opType);
  if (opType === "unknown") {
    return null;
  }
  return {
    id,
    opType,
    title,
    description,
    detail: typeof raw.detail === "string" ? raw.detail : "",
  };
}

export type ContextCompressionMode = "full" | "focused" | "compact" | "budgeted";
export type StepScope = "selection" | "active_file" | "open_files" | "workspace";
export type StepExecutionMode = "analyze" | "diff" | "test" | "fix";

/** Agent 角色 */
export type AgentRole = "architect" | "designer" | "coder" | "tester" | "reviewer";

/** Pipeline 阶段状态 */
export type PipelineStageStatus = "pending" | "active" | "completed" | "failed" | "paused";

/** Pipeline 阶段 */
export interface PipelineStage {
  role: AgentRole;
  name: string;
  status: PipelineStageStatus;
  pauseBefore?: boolean;
}

/** 单个步骤 */
export interface Step {
  id: string;
  title: string;
  type: "create" | "edit" | "run" | "test" | "analyze";
  status: "todo" | "doing" | "done" | "error" | "skipped";
  logs: string[];
  scope?: StepScope | null;
  executionMode?: StepExecutionMode | null;
}

/** Diff entry proposed by the Agent. */
export interface DiffEntry {
  id: string;
  file: string;
  baseHash?: string | null;
  provenance?: DiffProvenance | null;
  hunks: DiffHunk[];
  /**
   * `reverted` 是终态：工具写入的记录被撤销之后停在这里。
   *
   * 它不能退回 `pending` —— 那样 Apply 会把一条删除记录当成内容替换执行，把文件写成
   * 0 字节而不是删掉它；移动记录更是没有内容可应用。见后端 `undo_last_apply`。
   */
  status: "pending" | "partial" | "applied" | "rejected" | "failed" | "reverted";
  applyError?: string;
}

export interface DiffProvenance {
  protocol: string;
  operation: string;
  rationale?: string | null;
  schemaVersion?: number | null;
  changeIndex?: number | null;
  sourceRole?: string | null;
  sourceStage?: string | null;
  regeneratedFromDiffId?: string | null;
  regeneratedFromHunkIndex?: number | null;
  /** 移动过来的话，这里是移动前的工作区相对路径（`DiffEntry.file` 只有落点） */
  movedFrom?: string | null;
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  content: string;
  original: string;
  updated: string;
  provenance?: DiffHunkProvenance | null;
  status?: "pending" | "applied" | "rejected" | "failed" | "reverted";
  applyError?: string;
}

export interface DiffHunkProvenance {
  changeIndex?: number | null;
  hunkIndex?: number | null;
  sourceRole?: string | null;
  sourceStage?: string | null;
  promptContext?: string | null;
  rationale?: string | null;
}

export interface ApplyDiffError {
  diffId: string;
  file: string;
  message: string;
}

export interface ApplyDiffsResult {
  applied: DiffEntry[];
  failed: ApplyDiffError[];
}

export interface AgentActionLogEntry {
  id: string;
  timestamp: string;
  level: "info" | "warn" | "error" | "success";
  phase: string;
  role?: AgentRole | string | null;
  stage?: string | null;
  summary: string;
  details: string;
  contextSummary?: string | null;
  diffSummary?: string | null;
}

export interface SddArtifact {
  id: string;
  title: string;
  slug: string;
  frontmatter: Record<string, string>;
  markdown: string;
  sourceRunId?: string | null;
  reviewFindings: string[];
  status: "draft" | "reviewed" | "approved" | string;
}

export interface SavedSddArtifactResponse {
  path: string;
  artifact: SddArtifact;
}

export interface GhostSuggestion {
  id: string;
  title: string;
  detail: string;
  prompt: string;
  source: "problems" | "tasks" | "logs" | "workspace";
  createdAt: number;
}

/**
 * 上下文占用的**测量值**：来自供应商回报的最后一次请求。
 *
 * 和 `ContextEstimateResponse` 不是一回事，也不能相加或互相校对：估算是发送前按
 * 字符推出来的、只覆盖打包进去的那几节上下文；这个是真实 token，覆盖整个请求
 * （系统提示词、工具 schema、逐阶段累积的消息都在里面）。
 */
export interface ContextUsageMeasurement {
  /** 最后一次请求的输入 token */
  lastPromptTokens: number;
  /** 输入 + 输出。上下文窗口是两者共用的，所以占用要算总数 */
  lastTotalTokens: number;
}

/**
 * 当前这一轮任务。**只有标题和身份**，没有步骤列表。
 *
 * 步骤是 `steps`，那是后端 `record_plan` 的产物；任务只负责回答"现在在做哪件事"。
 * 曾经这里还有 `steps` / `affectedFiles` / `status` 三个字段，从来没有人写过它们，
 * 于是标题栏永远显示硬编码的 "Agent Task"。
 */
export interface Task {
  /** 发起这一轮的 run id，方便和后端日志对上 */
  id: string;
  /** 来源见 `deriveTaskTitle`：prompt 的第一行 */
  title: string;
}


/**
 * 后端真正会喂给模型的一轮对话。
 *
 * 和 `ChatMessage` 不是一回事，也不该被混在一起显示：消息流不设上限、不持久化、
 * 由各个调用点自己 push；这个列表只留末尾若干轮、每轮都截断过，而且只有一次
 * `send_agent_prompt` **成功**才会产生一轮。要让用户管上下文，得让他看见这一份。
 */
export interface ConversationTurn {
  /** 后端分配的稳定编号；切上下文时回传的就是它，不是下标 */
  id: string;
  prompt: string;
  outcome: string;
}

/**
 * 上一次连通性测试的结果。
 *
 * 和 `llmConfigured` 是两件事，不能混：后者只说明"存了一个 profile"，端点通不通、
 * key 对不对、模型名存不存在，它一概不知道。之前状态栏只有一个绿点表示"已配置"，
 * 而一个填错端点的 profile 也会让它变绿 —— 这正是这个项目明令禁止的那种虚假信心。
 *
 * `unknown` 是诚实的默认值：没测过就是不知道，不能算通。
 */
export interface LlmConnectionState {
  status: "unknown" | "ok" | "failed";
  /** 什么时候测的；`unknown` 时为 null */
  checkedAt: number | null;
  /** 成功时是模型回的一小段内容，失败时是错误原因 */
  detail: string | null;
  /**
   * 被测的那个目标的指纹（见 `stores/llmConnection.ts`）。
   *
   * 结果必须带着目标一起存：否则改完端点之后，上一次的 ok 会替一个从没连过的
   * 地址作保。
   */
  target: string | null;
}

/** Chat 消息 */
export interface ChatMessage {
  id: string;
  role: "user" | "agent" | "system";
  content: string;
  timestamp: number;
  files?: string[];
}

// ====== LLM 配置相关 ======

/** LLM 模型提供商 */
export type ModelProvider = "openai" | "anthropic" | "azure" | "deepseek" | "local" | "custom";

export type LocalModelType = "starcoder" | "codellama" | "deepseek-coder" | "codegemma";

/** LLM 配置 */
export interface LlmConfig {
  provider: ModelProvider;
  endpoint: string;
  apiKey: string;
  model: string;
  modelType?: LocalModelType;
  modelPath?: string;
  modelFile?: string;
  nThreads?: number;
  nCtx?: number;
  nGpuLayers?: number;
  nBatch?: number;
  temperature?: number;
  topP?: number;
  topK?: number;
  maxTokens?: number;
}

/** LLM 配置响应（apiKey 脱敏） */
export interface LlmConfigResponse {
  endpoint: string;
  api_key_masked: string;
  model: string;
  context_compression: ContextCompressionMode;
  profiles?: LlmProfile[];
  active_profile_id?: string;
}

export interface LlmProfile {
  id: string;
  name: string;
  provider: ModelProvider;
  endpoint: string;
  api_key_masked: string;
  /**
   * 这个 profile 现在能不能真的跑起来（后端判定，不要在前端重算）。
   *
   * 不能拿 `api_key_masked !== "not configured"` 当"已配置"：一份没被开启的明文密钥
   * 会显示成 `sk-1****7890 (plaintext in config.json)`，字符串比较判成已配置，而每次
   * 运行都会失败。旧的可选性是为了兼容后端还没带这个字段的旧响应。
   */
  api_key_usable?: boolean;
  model: string;

  modelType?: LocalModelType;
  modelPath?: string;
  modelFile?: string;
  nThreads?: number;
  nCtx?: number;
  nGpuLayers?: number;
  nBatch?: number;
  temperature?: number;
  topP?: number;
  topK?: number;
  maxTokens?: number;
  maxContextTokens?: number;
  reservedOutputTokens?: number;
  maxOutputTokens?: number;
  /** 单次运行的 token 上限；未设置或 0 表示不限制 */
  maxRunTokens?: number;
  /** 每百万 prompt token 的价格，单位微美元（$0.28/M = 280000） */
  promptMicrosPerMillion?: number;
  completionMicrosPerMillion?: number;
  /** 单次运行的金额上限（微美元）；两个价格都配齐才会被执行 */
  maxRunSpendMicros?: number;
  effectiveInputTokens?: number;
  toolCallMode?: "text_protocol" | "native_tools";
}

export interface LlmProfilesResponse {
  profiles: LlmProfile[];
  active_profile_id: string;
  context_compression: ContextCompressionMode;
}

export interface ContextEstimateSection {
  id: string;
  label: string;
  chars: number;
  estimatedTokens: number;
  included: boolean;
  trimmed: boolean;
  excludedReason?: string | null;
}

export interface ContextEstimateResponse {
  sections: ContextEstimateSection[];
  rawChars: number;
  finalChars: number;
  estimatedTokens: number;
  inputBudgetTokens?: number | null;
  trimmed: boolean;
}

export interface SaveLlmProfileRequest {
  id?: string;
  name: string;
  provider: ModelProvider;
  endpoint: string;
  apiKey?: string;
  model: string;
  modelType?: LocalModelType;
  modelPath?: string;
  modelFile?: string;
  nThreads?: number;
  nCtx?: number;
  nGpuLayers?: number;
  nBatch?: number;
  temperature?: number;
  topP?: number;
  topK?: number;
  maxTokens?: number;
  maxContextTokens?: number;
  reservedOutputTokens?: number;
  maxOutputTokens?: number;
  maxRunTokens?: number;
  promptMicrosPerMillion?: number;
  completionMicrosPerMillion?: number;
  maxRunSpendMicros?: number;
  toolCallMode?: "text_protocol" | "native_tools";
  setActive?: boolean;
}

/** 模型提供商预设 */
export interface ProviderPreset {
  id: ModelProvider;
  label: string;
  defaultEndpoint: string;
  defaultModel: string;
  models: string[];
  defaultMaxContextTokens?: number;
  defaultReservedOutputTokens?: number;
  defaultMaxOutputTokens?: number;
  defaultToolCallMode?: "text_protocol" | "native_tools";
}
