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
}

/** `read-only` 预设：既不新建文件，也不跑命令。 */
export const READ_ONLY_PERMISSIONS: AgentPermission = {
  allowFileCreate: false,
  allowCommandRun: false,
};

/** `create-files` 预设：可以新建文件（改动仍进审查区），但不跑命令。 */
export const CREATE_FILES_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowCommandRun: false,
};

/** `run-commands` 预设：可以新建文件，也可以跑项目自己声明的命令。 */
export const RUN_COMMANDS_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowCommandRun: true,
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

/** 破坏性操作类型 */
export type DestructiveOpType = "file_delete" | "command_run" | "git_push" | "git_force";

export interface DestructiveOpConfirm {
  id: string;
  opType: DestructiveOpType;
  title: string;
  description: string;
  detail: string;
  requireExplicitConfirm: boolean;
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
  status: "pending" | "partial" | "applied" | "rejected" | "failed";
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
  status?: "pending" | "applied" | "rejected" | "failed";
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

/** Task 任务 */
export interface Task {
  id: string;
  title: string;
  status: "todo" | "doing" | "done" | "error";
  steps: Step[];
  affectedFiles: string[];
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
