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

/** Agent 控制模式 */
export type AgentMode = "suggest" | "edit" | "auto";

/** IDE 工作模式，独立于 Agent 权限模式 */
export type IdeMode = "code" | "plan";

/** Agent 权限预设 */
export type AgentPermissionPreset = "ask" | "suggest" | "auto";

/** 细粒度 Agent 权限 */
export interface AgentPermission {
  allowFileCreate: boolean;
  allowFileDelete: boolean;
  allowCommandRun: boolean;
  allowGitActions: boolean;
}

/** 默认权限（ask 模式下的全手动确认） */
export const DEFAULT_PERMISSIONS: AgentPermission = {
  allowFileCreate: false,
  allowFileDelete: false,
  allowCommandRun: false,
  allowGitActions: false,
};

/** suggest 预设：审查但不自动执行 */
export const SUGGEST_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowFileDelete: false,
  allowCommandRun: false,
  allowGitActions: false,
};

/** auto 预设：全部放行 */
export const AUTO_PERMISSIONS: AgentPermission = {
  allowFileCreate: true,
  allowFileDelete: true,
  allowCommandRun: true,
  allowGitActions: true,
};

/** 根据预设获取权限 */
export function permissionsForPreset(preset: AgentPermissionPreset): AgentPermission {
  switch (preset) {
    case "ask": return { ...DEFAULT_PERMISSIONS };
    case "suggest": return { ...SUGGEST_PERMISSIONS };
    case "auto": return { ...AUTO_PERMISSIONS };
  }
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
export type ModelProvider = "openai" | "anthropic" | "azure" | "deepseek" | "custom";

/** LLM 配置 */
export interface LlmConfig {
  provider: ModelProvider;
  endpoint: string;
  apiKey: string;
  model: string;
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
  maxContextTokens?: number;
  reservedOutputTokens?: number;
  maxOutputTokens?: number;
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
  maxContextTokens?: number;
  reservedOutputTokens?: number;
  maxOutputTokens?: number;
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
