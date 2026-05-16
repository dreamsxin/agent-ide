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

export type ContextCompressionMode = "full" | "focused" | "compact";

/** Agent 角色 */
export type AgentRole = "architect" | "coder" | "tester" | "reviewer";

/** Pipeline 阶段状态 */
export type PipelineStageStatus = "pending" | "active" | "completed" | "failed";

/** Pipeline 阶段 */
export interface PipelineStage {
  role: AgentRole;
  name: string;
  status: PipelineStageStatus;
}

/** 单个步骤 */
export interface Step {
  id: string;
  title: string;
  type: "create" | "edit" | "run" | "test";
  status: "todo" | "doing" | "done" | "error";
  logs: string[];
}

/** Diff entry proposed by the Agent. */
export interface DiffEntry {
  id: string;
  file: string;
  baseHash?: string | null;
  provenance?: DiffProvenance | null;
  hunks: DiffHunk[];
  status: "pending" | "applied" | "rejected" | "failed";
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
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  content: string;
  original: string;
  updated: string;
  status?: "pending" | "applied" | "rejected" | "failed";
  applyError?: string;
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
}

export interface LlmProfilesResponse {
  profiles: LlmProfile[];
  active_profile_id: string;
  context_compression: ContextCompressionMode;
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
}
