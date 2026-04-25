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

/** Diff 条目 */
export interface DiffEntry {
  id: string;
  file: string;
  hunks: DiffHunk[];
  status: "pending" | "applied" | "rejected";
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  content: string;
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
export type ModelProvider = "openai" | "anthropic" | "azure" | "custom";

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
}

/** 模型提供商预设 */
export interface ProviderPreset {
  id: ModelProvider;
  label: string;
  defaultEndpoint: string;
  defaultModel: string;
  models: string[];
}
