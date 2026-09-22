import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "../utils/tauri";
import { deriveTaskTitle } from "../utils/agentTaskTitle";
import {
  normalizeExternalActions,
  type ExternalActionRecord,
} from "../utils/externalActions";
import type {
  AgentState,
  AgentMode,
  AgentSessionSummary,
  IdeMode,
  AgentRole,
  ContextCompressionMode,
  ContextUsageMeasurement,
  PipelineStage,
  LlmConfigResponse,
  LlmConnectionState,
  LlmProfile,
  LlmProfilesResponse,
  SaveLlmProfileRequest,
  Task,
  Step,
  DiffEntry,
  ApplyDiffsResult,
  ChatMessage,
  ConversationTurn,
  ContextEstimateResponse,
  SddArtifact,
  SavedSddArtifactResponse,
  GhostSuggestion,
  AgentPermission,
  BooleanPermissionKey,
  AgentPermissionPreset,
  DestructiveOpConfirm,
  RunUsage,
} from "../types/agent";
import {
  llmTargetFingerprint,
  UNVERIFIED_LLM_CONNECTION,
} from "./llmConnection";
import {
  mcpApprovalForPermissions,
  normalizeAgentMode,
  normalizeAgentSessionDetail,
  normalizeAgentSessionList,
  permissionsForPreset,
  READ_ONLY_PERMISSIONS,
} from "../types/agent";

interface AgentStore {
  // ====== Agent 状态 ======
  state: AgentState;
  mode: AgentMode;
  ideMode: IdeMode;
  currentTask: Task | null;
  /**
   * 上下文占用的测量值，来自后端 `agent-context-usage`。
   *
   * 和 ChatView 里那份发送前的估算刻意分开：单位和覆盖范围都不同，混成一个数字会让
   * 人以为它们可以互相校对。没有一次请求回报过用量时保持 null —— 界面这时什么都不
   * 显示，而不是显示 0%。
   */
  contextUsage: ContextUsageMeasurement | null;
  diffs: DiffEntry[];
  /**
   * 本次运行里撤不回的外部动作（浏览器导航等）。
   *
   * 和 `diffs` 并列而不是塞进它：diff 有 `previous` 和撤销入口，这些没有。混在一起
   * 会让审查区里的"撤销"对其中一半是假的。
   */
  externalActions: ExternalActionRecord[];
  sddArtifacts: SddArtifact[];
  activeSddArtifact: SddArtifact | null;
  ghostSuggestions: GhostSuggestion[];
  steps: Step[];
  error: string | null;
  lastApplyResult: ApplyDiffsResult | null;
  /**
   * 当前可撤销的那次应用，`null` 表示没有退路。
   *
   * 由后端查询而来，不从 diff 状态推断：回滚栈在 orchestrator 内存里，进程重启
   * 就没了，推断会在重启后显示一个点下去必然失败的 Undo 按钮。
   */
  pendingUndo: { label: string; files: string[] } | null;
  /** 本次运行至今的 token/花费，随 `agent-state-changed` 更新；null 表示还没有记账器 */
  runUsage: RunUsage | null;
  streamContent: string;
  isStreaming: boolean;
  agentRunId: string | null;
  restoredSession: AgentRestoredSession | null;

  // ====== Chat 消息 ======
  messages: ChatMessage[];
  /**
   * 后端此刻真正会喂给模型的几轮对话。
   *
   * 刻意和 `messages` 分开存：那是界面记录，这是上下文本体，两者会因为
   * "只在成功时记一轮"、"只留末尾 6 轮"、"刷新后消息没了而后端还在"而不一致。
   * 想让用户管上下文，就得让他看见后者。空数组既表示"真的没有"也表示"还没查过"，
   * 因为这两种情况下界面要显示的东西一样。
   */
  conversationTurns: ConversationTurn[];
  /**
   * 这个工作区的历史会话，最近更新的在前。由 `loadSessions` 从磁盘读回。
   *
   * 一个"会话"就是那几轮对话（模型上下文），不含 steps / diffs —— 见
   * `AgentSessionSummary`。界面上的措辞必须照这个事实来。
   */
  sessions: AgentSessionSummary[];
  /** 后端此刻在用的那个会话 id，列表据它高亮 */
  activeSessionId: string;
  /** 会话历史写不进磁盘（或读不出来）时的那句话；正常时为 null */
  sessionWarning: string | null;
  /** 这个环境此刻会不会存会话。false 时历史面板要说清"这里不保存"，而不是显示一个空列表 */
  sessionsAreSaved: boolean;




  // ====== 角色与流水线 ======
  activeRole: AgentRole;
  pipeline: PipelineStage[];

  // ====== LLM 配置 ======
  llmConfigured: boolean;
  /**
   * 上一次连通性测试的结果。
   *
   * 单独一份状态，是因为它和 `llmConfigured` 回答的不是同一个问题：那个只说明存了
   * profile，端点通不通它不知道。端点 / 模型 / 活动 profile 一变就回到 `unknown`，
   * 否则一个绿点会替一个全新的、没验证过的端点作保。
   */
  llmConnection: LlmConnectionState;
  llmEndpoint: string;
  llmModel: string;
  apiKeyMasked: string;
  contextCompression: ContextCompressionMode;
  llmProfiles: LlmProfile[];
  activeProfileId: string;
  chatProfileId: string | null;
  chatContextCompression: ContextCompressionMode | null;

  // ====== 权限控制 ======
  permissionPreset: AgentPermissionPreset;
  permissions: AgentPermission;
  pendingConfirm: DestructiveOpConfirm | null;

  // ====== 同步 Actions ======
  setState: (state: AgentState) => void;
  setMode: (mode: AgentMode) => void;
  setIdeMode: (mode: IdeMode) => void;
  /** 后端回报的上下文占用；`null` 表示这次运行没有任何可测量的用量 */
  setContextUsage: (usage: ContextUsageMeasurement | null) => void;
  setSteps: (steps: Step[]) => void;
  updateStep: (stepId: string, updates: Partial<Step>) => void;
  setDiffs: (diffs: DiffEntry[]) => void;
  setSddArtifact: (artifact: SddArtifact) => void;
  updateActiveSddMarkdown: (markdown: string) => void;
  saveActiveSdd: (overwrite?: boolean) => Promise<SavedSddArtifactResponse | null>;
  promoteSddToCodePrompt: () => void;
  setGhostSuggestions: (suggestions: GhostSuggestion[]) => void;
  dismissGhostSuggestion: (id: string) => void;
  restoreDiffs: (workspacePath?: string) => Promise<void>;
  /** 从后端读回撤不回的外部动作（`get_agent_external_actions`）。 */
  refreshExternalActions: () => Promise<void>;
  /** 忘掉更早会话留下的外部动作记录；这一次会话的不受影响。 */
  forgetEarlierExternalActions: () => Promise<void>;
  restoreAgentSession: (workspacePath?: string) => void;
  reconcileBackendRun: () => Promise<void>;
  /**
   * 开一个新会话：界面上的任务、步骤、SDD 全部清掉，**后端换一个会话 id 并清空对话历史**。
   *
   * 后端那半边必须一起清。少清它的话，界面看着是全新开始，而下一条提问仍然带着上一个
   * 任务的 `conversation_digest()` 进模型上下文 —— 用户看不见，也没法解释模型为什么在
   * 接着聊上一件事。
   *
   * 清掉的那一份不会丢：它以一个会话的形式留在磁盘上，`sessions` 里能点回去。
   */
  startNewSession: () => Promise<void>;
  /** 从磁盘读回这个工作区的历史会话。历史面板显示之前必须先调。 */
  loadSessions: () => Promise<void>;
  /**
   * 回到一个历史会话。
   *
   * 只换回**上下文**：steps / diffs 不恢复（diff 描述的是磁盘某一刻的样子，隔天多半已经
   * 对不上）。所以这里顺手把审查区和计划清成空的，而不是留着上一个会话的 —— 留着会让界面
   * 显示的和后端实际的不一致，那正是这个产品要避免的。
   */
  resumeSession: (sessionId: string) => Promise<void>;
  /** 删掉一个历史会话。删的是当前这个时，后端会顺带换一个新的。 */
  deleteSession: (sessionId: string) => Promise<void>;


  setPipeline: (stages: PipelineStage[]) => void;
  addDiff: (diff: DiffEntry) => void;
  markDiffApplied: (diffId: string) => void;
  markDiffRejected: (diffId: string) => void;
  setError: (error: string | null) => void;
  appendStreamContent: (token: string) => void;
  clearStreamContent: () => void;
  addMessage: (msg: ChatMessage) => void;
  updateMessage: (id: string, updates: Partial<ChatMessage>) => void;
  /** 从后端拉一次真正的上下文；界面要显示它之前必须先调 */
  loadConversationTurns: () => Promise<void>;
  truncateConversationFrom: (turnId: string) => Promise<void>;



  // ====== 异步 Actions (IPC) ======
  sendPrompt: (params: {
    prompt: string;
    contextFiles?: string[];
    activeFile?: string;
    activeFileContent?: string;
    selection?: string;
    profileId?: string;
    contextCompression?: ContextCompressionMode;
    contextSources?: {
      includeProjectTree?: boolean;
      includeGitDiff?: boolean;
      includeProjectMemory?: boolean;
    };
    ideMode?: IdeMode;
    /**
     * IDE 当下的运行状况（问题、终端、失败的检查、日志）。
     *
     * 独立字段而不是拼进 `prompt`：拼进去的话它绕过后端的上下文估算和预算裁剪，
     * 面板上那个"selected context N tokens"就会系统性少算上万字符。
     */
    ideRuntime?: string | null;
  }) => Promise<void>;
  stopAgent: () => Promise<void>;
  changeMode: (mode: AgentMode) => Promise<void>;
  applyAllDiffs: () => Promise<DiffEntry[]>;
  /** 撤销最近一次应用，把文件恢复到那次应用之前；返回是否全部恢复成功 */
  undoLastApply: () => Promise<boolean>;
  /** 向后端确认现在有没有可撤销的应用。只用于首次挂载取初值；之后由 agent-state-changed 推送 */
  refreshPendingUndo: () => Promise<void>;
  /** 由 agent-state-changed 的 payload 驱动 */
  setPendingUndo: (undo: { label: string; files: string[] } | null) => void;
  setRunUsage: (usage: RunUsage | null) => void;
  applyDiff: (diffId: string) => Promise<DiffEntry[]>;
  applyDiffHunk: (diffId: string, hunkIndex: number) => Promise<DiffEntry[]>;
  clearApplyResult: () => void;
  rejectAllDiffs: () => Promise<DiffEntry[]>;
  rejectDiff: (diffId: string) => Promise<DiffEntry | null>;
  rejectDiffHunk: (diffId: string, hunkIndex: number) => Promise<DiffEntry | null>;
  estimateContext: (params: AgentContextParams) => Promise<ContextEstimateResponse | null>;
  updateAgentStep: (step: Step) => Promise<Step | null>;
  updateAgentSteps: (steps: Step[]) => Promise<Step[]>;
  skipAgentStep: (stepId: string) => Promise<Step | null>;
  runAgentStep: (params: AgentStepRunParams) => Promise<void>;
  continueAgentPipeline: () => Promise<void>;
  regenerateDiff: (params: RegenerateDiffParams) => Promise<void>;

  // ====== 模型配置 ======
  fetchLlmConfig: () => Promise<void>;
  saveLlmProfile: (request: SaveLlmProfileRequest) => Promise<void>;
  deleteLlmProfile: (profileId: string) => Promise<void>;
  setActiveLlmProfile: (profileId: string) => Promise<void>;
  setChatProfileId: (profileId: string | null) => void;
  revealLlmApiKey: (profileId?: string | null) => Promise<string>;
  setChatContextCompression: (mode: ContextCompressionMode | null) => void;
  updateContextCompression: (mode: ContextCompressionMode) => Promise<void>;

  // ====== 角色管理 ======
  setActiveRole: (role: AgentRole) => Promise<void>;
  fetchActiveRole: () => Promise<void>;

  // ====== 流水线管理 ======
  fetchPipeline: () => Promise<void>;
  updatePipeline: (stages: PipelineStage[]) => Promise<void>;
  resetPipeline: () => Promise<void>;

  // ====== 权限管理 ======
  setPermissionPreset: (preset: AgentPermissionPreset) => void;
  togglePermission: (key: BooleanPermissionKey) => void;
  setBrowserOrigins: (origins: string[]) => void;
  setPageReadOrigins: (origins: string[]) => void;
  setInputApps: (apps: string[]) => void;
  /** 设置允许被观察的应用清单（可执行文件名）。空清单等于不许观察。 */
  setComputerApps: (apps: string[]) => void;
  setCaptureApps: (apps: string[]) => void;
  requestConfirm: (confirm: DestructiveOpConfirm) => void;
  /**
   * 把决定送回后端，收掉对话框。
   *
   * 返回后端是否真的还有人在等：超时之后才点到的话是 false，界面不该显示"已批准"。
   */
  resolveConfirm: (approved: boolean) => Promise<boolean>;
  /** 后端已经不等这条请求了（超时 / Stop）。只在 id 对得上时收掉对话框。 */
  closeConfirm: (requestId: string) => void;

  // ====== 连通性测试 ======
  testLlmConnection: () => Promise<string>;
}

interface AgentContextParams {
  contextFiles?: string[];
  activeFile?: string;
  activeFileContent?: string;
  selection?: string;
  profileId?: string;
  contextCompression?: ContextCompressionMode;
  contextSources?: {
    includeProjectTree?: boolean;
    includeGitDiff?: boolean;
    includeProjectMemory?: boolean;
  };
  ideMode?: IdeMode;
  /**
   * 估算要和真正发出去的一致，所以这一段也得给后端；见 `sendPrompt` 的同名字段。
   *
   * 只属于"整轮 prompt"这两条路径。流水线单步不接它（后端那边固定传 None），所以
   * `AgentStepRunParams` 把它 Omit 掉 —— 类型上收得下、实际被丢弃的字段，和一个按了
   * 没反应的开关是同一种东西。
   */
  ideRuntime?: string | null;
}

interface AgentStepRunParams extends Omit<AgentContextParams, "ideRuntime"> {
  step: Step;
  extraPrompt?: string;
  regeneratedFromDiffId?: string;
  regeneratedFromHunkIndex?: number;
}

interface RegenerateDiffParams extends AgentContextParams {
  diff: DiffEntry;
  hunkIndex?: number;
  currentFileContent?: string;
}

interface AgentRestoredSession {
  runId: string | null;
  restoredAt: number;
  interrupted: boolean;
  backendMatched: boolean | null;
  updatedAt?: number;
}

interface AgentStatusResponse {
  state: AgentState;
  mode: AgentMode;
  ideMode?: IdeMode;
  currentRunId?: string | null;
  lastRunId?: string | null;
}

const DEFAULT_PIPELINE: PipelineStage[] = [
  { role: "architect", name: "Design", status: "pending" },
  { role: "coder", name: "Implement", status: "pending" },
  { role: "tester", name: "Test", status: "pending" },
  { role: "reviewer", name: "Review", status: "pending" },
];

/**
 * 空聊天区里那条欢迎消息。
 *
 * 它同时是唯一的"这东西怎么用"说明：面板上四个页签加两个图标按钮，没有任何地方解释它们
 * 的关系，而用户第一句话正是"不知道 task 和 plan 怎么配合、怎么新开、怎么看历史"。所以这
 * 一条按流程写，而不是问好。
 *
 * 只有一处定义：初始状态和"新建任务"曾经写着两句不同的话，同一个"刚开始"的界面因此有两种样子。
 */
function welcomeMessage(): ChatMessage {
  return {
    id: "welcome",
    role: "system",
    content:
      "Describe what you want changed and send it. I break it into steps — they show up under **Plan**, " +
      "where you can run, retry or skip one. Anything I write to files queues under **Changes** for you to " +
      "review or undo. **New task** starts a fresh one; **Task history** brings an earlier one back.",
    timestamp: Date.now(),
  };
}



export const useAgentStore = create<AgentStore>((set, get) => ({
  // ========== 初始值 ==========
  state: "idle",
  mode: "suggest",
  ideMode: "code",
  currentTask: null,
  contextUsage: null,
  diffs: [],
  externalActions: [],
  sddArtifacts: [],
  activeSddArtifact: null,
  ghostSuggestions: [],
  steps: [],
  error: null,
  lastApplyResult: null,
  pendingUndo: null,
  runUsage: null,
  streamContent: "",
  isStreaming: false,
  agentRunId: null,
  restoredSession: null,
  conversationTurns: [],
  sessions: [],
  activeSessionId: "",
  sessionWarning: null,
  sessionsAreSaved: false,
  messages: [welcomeMessage()],

  activeRole: "coder",
  pipeline: DEFAULT_PIPELINE,
  llmConfigured: false,
  llmConnection: UNVERIFIED_LLM_CONNECTION,
  llmEndpoint: "",
  llmModel: "",
  apiKeyMasked: "",
  contextCompression: "focused",
  llmProfiles: [],
  activeProfileId: "",
  chatProfileId: null,
  chatContextCompression: null,

  // ====== 权限初始值 ======
  // 预设和权限必须同源。以前这里是 `"suggest"` 配 `DEFAULT_PERMISSIONS`，
  // 也就是界面高亮着"可以新建文件"那一档，而开关实际是关的。
  permissionPreset: "read-only",
  permissions: READ_ONLY_PERMISSIONS,
  pendingConfirm: null,

  // ========== 同步 Actions ==========
  setState: (state) => {
    set({ state });
    persistAgentSession(get());
  },
  setMode: (mode) => {
    set({ mode });
    persistAgentSession(get());
  },
  setIdeMode: (ideMode) => {
    set({ ideMode });
    persistAgentSession(get());
  },
  setContextUsage: (contextUsage) => set({ contextUsage }),
  setSteps: (steps) => {
    set({ steps });
    persistAgentSession(get());
  },
  setPipeline: (pipeline) => {
    set({ pipeline });
    persistAgentSession(get());
  },
  updateStep: (stepId, updates) =>
    set((s) => {
      const nextSteps = s.steps.map((st) =>
        st.id === stepId ? { ...st, ...updates } : st
      );
      windowQueuePersist(() => persistAgentSession({ ...get(), steps: nextSteps }));
      return { steps: nextSteps };
    }),
  setDiffs: (diffs) => {
    persistDiffs(diffs);
    set({ diffs, lastApplyResult: null });
  },
  setSddArtifact: (artifact) =>
    set((state) => {
      const sddArtifacts = [
        ...state.sddArtifacts.filter((item) => item.id !== artifact.id),
        artifact,
      ].slice(-50);
      const next = { sddArtifacts, activeSddArtifact: artifact };
      windowQueuePersist(() => persistAgentSession({ ...get(), ...next }));
      return next;
    }),
  updateActiveSddMarkdown: (markdown) =>
    set((state) => {
      if (!state.activeSddArtifact) return {};
      const activeSddArtifact = { ...state.activeSddArtifact, markdown };
      const sddArtifacts = state.sddArtifacts.map((item) =>
        item.id === activeSddArtifact.id ? activeSddArtifact : item
      );
      return { activeSddArtifact, sddArtifacts };
    }),
  saveActiveSdd: async (overwrite = false) => {
    const artifact = get().activeSddArtifact;
    if (!artifact) return null;
    if (!isTauriRuntime()) {
      throw new Error("SDD save is available in the Tauri app runtime.");
    }
    const response = await invoke<SavedSddArtifactResponse>("save_sdd_artifact", {
      request: { artifact, overwrite },
    });
    get().setSddArtifact({ ...response.artifact, status: "approved" });
    return response;
  },
  promoteSddToCodePrompt: () => {
    const artifact = get().activeSddArtifact;
    if (!artifact) return;
    set({ ideMode: "code" });
    get().addMessage({
      id: `sdd-code-${Date.now()}`,
      role: "system",
      content: `SDD approved for code mode: ${artifact.title}\n\n${artifact.markdown}`,
      timestamp: Date.now(),
    });
  },
  setGhostSuggestions: (ghostSuggestions) => set({ ghostSuggestions }),
  dismissGhostSuggestion: (id) =>
    set((state) => ({
      ghostSuggestions: state.ghostSuggestions.filter((item) => item.id !== id),
    })),
  restoreDiffs: async (workspacePath) => {
    const persisted = loadDiffs(workspacePath);
    set({ diffs: persisted, lastApplyResult: null });
    if (!isTauriRuntime()) return;
    try {
      // 后端 orchestrator 的 diff 只在内存里，重启后必然是空的；前端却从
      // localStorage 恢复。不对账的话界面会摆出一排点了没反应的 Apply 按钮。
      const backend = await invoke<DiffEntry[]>("get_agent_diffs");
      if (backend.length > 0) {
        set({ diffs: backend });
        persistDiffs(backend);
        return;
      }
      const orphaned = persisted.filter(isReviewableDiff);
      if (orphaned.length > 0) {
        set({
          error: `${orphaned.length} restored change(s) are no longer known to the backend and cannot be applied. Re-run the task to regenerate them.`,
        });
      }
    } catch (err: unknown) {
      console.warn("[AgentStore] get_agent_diffs failed:", err);
    }
  },
  refreshExternalActions: async () => {
    if (!isTauriRuntime()) return;
    try {
      // 撤不回的动作在后端有一份，和 diff 同级；只靠那条 action-log 事件的话，
      // 刷新一次界面就再也看不到"这次运行动了外面什么"。
      const backend = await invoke<unknown>("get_agent_external_actions");
      set({ externalActions: normalizeExternalActions(backend) });
    } catch (err: unknown) {
      console.warn("[AgentStore] get_agent_external_actions failed:", err);
    }
  },
  forgetEarlierExternalActions: async () => {
    if (!isTauriRuntime()) return;
    try {
      // 后端只会忘掉更早会话的那些，并在磁盘上留一条墓碑；所以这里不自己删本地数组，
      // 而是重新读一遍 —— 界面上剩下什么，由那份唯一的记录说了算。
      await invoke("forget_earlier_external_actions");
      await get().refreshExternalActions();
    } catch (err: unknown) {
      // 清不掉要说出来：静默失败会让用户以为记录没了，而它还在磁盘上
      set({ error: `Could not clear the earlier external action records: ${String(err)}` });
    }
  },
  restoreAgentSession: (workspacePath) => {
    const restored = loadAgentSession(workspacePath);
    if (!restored) return;
    set(restored);
  },
  reconcileBackendRun: async () => {
    const restored = get().restoredSession;
    if (!restored || !isTauriRuntime()) return;
    try {
      const status = await invoke<AgentStatusResponse>("get_agent_state");
      const matched = Boolean(
        restored.runId &&
        (status.currentRunId === restored.runId || status.lastRunId === restored.runId)
      );
      set((state) => ({
        ideMode: status.ideMode ?? state.ideMode,
        restoredSession: state.restoredSession
          ? { ...state.restoredSession, backendMatched: matched }
          : null,
      }));
      // 步骤和 SDD 也要对账，理由和 `restoreDiffs` 一样：它们在后端只活在内存里，前端却从
      // localStorage 恢复。不对账的话，Run/Skip 会打到一个后端根本不认识的步骤上 —— 那是
      // 一排点了报错的按钮，而"界面显示的和后端实际的不一致"正是这个产品要避免的。
      //
      // 对上了就照抄后端，**包括抄一个空列表**：那正是"后端这次重新规划成了空"的情况，而
      // 界面上还留着上一个任务的步骤 —— 这时横幅写的是"Backend run matched"，用户没有任何
      // 理由怀疑那几行是假的。对不上才保留恢复出来的那份，横幅也已经在说明它可能对不上。
      //
      // 两次调用都把命令名写成字面量、各自 try/catch，而不是抽一个 `reconcile(command)`
      // 辅助函数：名字一进变量，`tests/ipc-contract.test.ts` 那条"注册了却没人调"的扫描
      // 就看不见它了 —— 这个仓库每一处按名字查的工具都一样。
      try {
        const steps = await invoke<Step[]>("get_agent_steps");
        if (matched || steps.length > 0) set({ steps });
      } catch (err: unknown) {
        console.warn("[AgentStore] get_agent_steps reconciliation failed:", err);
      }
      try {
        const sddArtifacts = await invoke<SddArtifact[]>("get_agent_sdd_artifacts");
        if (matched || sddArtifacts.length > 0) set({ sddArtifacts });
      } catch (err: unknown) {
        console.warn("[AgentStore] get_agent_sdd_artifacts reconciliation failed:", err);
      }
      persistAgentSession(get());



    } catch (err) {
      console.warn("[AgentStore] get_agent_state reconciliation failed:", err);
      set((state) => ({
        restoredSession: state.restoredSession
          ? { ...state.restoredSession, backendMatched: false }
          : null,
      }));
    }
  },
  startNewSession: async () => {
    // 先问后端，成功了才清界面。反过来的话，后端拒绝（运行还在跑）时界面已经变成一个空的、
    // 空闲的新会话，而那次运行仍然带着旧上下文在跑 —— 连 Stop 按钮都跟着消失了。
    if (isTauriRuntime()) {
      try {
        const list = normalizeAgentSessionList(await invoke("start_new_agent_session"));
        set({
          sessions: list.sessions,
          activeSessionId: list.activeId,
          sessionWarning: list.warning,
          sessionsAreSaved: list.sessionsAreSaved,
        });
      } catch (err: unknown) {
        // 拒绝要说出来：清了界面却没清上下文，这件事在界面上完全看不出来
        set({ error: `Could not start a new session: ${String(err)}` });
        throw err;
      }
    }
    clearPersistedAgentSession();
    set({
      state: "idle",
      currentTask: null,
      contextUsage: null,
      steps: [],
      pipeline: DEFAULT_PIPELINE,
      sddArtifacts: [],
      activeSddArtifact: null,
      ghostSuggestions: [],
      error: null,
      streamContent: "",
      isStreaming: false,
      agentRunId: null,
      restoredSession: null,
      conversationTurns: [],
      messages: [welcomeMessage()],
    });
  },
  loadSessions: async () => {
    if (!isTauriRuntime()) return;
    try {
      const list = normalizeAgentSessionList(await invoke("list_agent_sessions"));
      set({
        sessions: list.sessions,
        activeSessionId: list.activeId,
        sessionWarning: list.warning,
        sessionsAreSaved: list.sessionsAreSaved,
      });
    } catch (err) {
      // 读不到历史不该让面板炸掉：这是一个列表展示，不是运行的一部分
      console.warn("[AgentStore] list_agent_sessions failed:", err);
    }
  },
  resumeSession: async (sessionId) => {
    if (!isTauriRuntime()) return;
    // 不 try/catch：运行中换会话、记录已经不在磁盘上，都是用户必须看到的拒绝，
    // 由调用方（历史面板）就地显示。吞掉的话点了就像没反应。
    const detail = normalizeAgentSessionDetail(
      await invoke("resume_agent_session", { sessionId })
    );
    if (!detail) return;
    clearPersistedAgentSession();
    set({
      state: "idle",
      // 任务标题跟着会话走：留着上一个会话的标题会让面板顶上写着另一件事
      currentTask: { id: detail.id, title: detail.title },
      contextUsage: null,
      // 计划不恢复，所以清空而不是留着上一个会话的那几步 —— 留着的话 Run/Skip 会打到一个
      // 后端已经不认识的步骤上。**diffs 刻意保留**：那是真实存在的待审查改动，磁盘上就是
      // 那样，换会话不该让它们从审查区消失（消失才是这个产品要避免的那种不一致）。
      steps: [],
      pipeline: DEFAULT_PIPELINE,
      sddArtifacts: [],
      activeSddArtifact: null,
      error: null,
      streamContent: "",
      isStreaming: false,
      agentRunId: null,
      restoredSession: null,
      conversationTurns: detail.turns,
      activeSessionId: detail.id,
      // 聊天区按恢复出来的几轮重建：空着的话用户看不出上下文里到底有什么。
      // 派生轮（跑某一步产生的）画成 agent 消息而不是 user —— 把 `Ran step: 加测试`
      // 画成用户消息等于告诉他那句话是他自己打的。
      messages: [
        {
          id: `resumed-${detail.id}`,
          role: "system" as const,
          content:
            `Resumed task "${detail.title}" — ${detail.turns.length} turn(s) of context are back. ` +
            "The plan from that task is not restored, and pending changes in the review area are left as they are.",
          timestamp: Date.now(),
        },
        ...detail.turns.flatMap((turn) => [
          {
            id: `${turn.id}-prompt`,
            role: turn.derived ? ("agent" as const) : ("user" as const),
            content: turn.prompt,
            timestamp: Date.now(),
          },
          {
            id: `${turn.id}-outcome`,
            role: "agent" as const,
            content: turn.outcome,
            timestamp: Date.now(),
          },
        ]),
      ],
    });
    await get().loadSessions();
  },
  deleteSession: async (sessionId) => {
    if (!isTauriRuntime()) return;
    const wasActive = get().activeSessionId === sessionId;
    const list = normalizeAgentSessionList(
      await invoke("delete_agent_session", { sessionId })
    );
    set({
      sessions: list.sessions,
      activeSessionId: list.activeId,
      sessionWarning: list.warning,
      sessionsAreSaved: list.sessionsAreSaved,
    });
    // 删掉的是正在用的那个：后端已经换了新会话，界面上那几轮也必须跟着消失，
    // 否则聊天区还列着一段模型此刻根本看不到的历史
    if (wasActive) {
      set({
        conversationTurns: [],
        currentTask: null,
        messages: [welcomeMessage()],
      });
    }
  },



  addDiff: (diff) =>
    set((s) => {
      const diffs = [...s.diffs, diff];
      persistDiffs(diffs);
      return { diffs };
    }),
  markDiffApplied: (diffId) =>
    set((s) => {
      const diffs = s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "applied" as const, applyError: undefined } : d
      );
      persistDiffs(diffs);
      return { diffs };
    }),
  markDiffRejected: (diffId) =>
    set((s) => {
      const diffs = s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "rejected" as const, applyError: undefined } : d
      );
      persistDiffs(diffs);
      return { diffs };
    }),
  setError: (error) => {
    set({ error, state: error ? "error" : "idle" });
    persistAgentSession(get());
  },
  clearApplyResult: () => set({ lastApplyResult: null }),
  appendStreamContent: (token) =>
    set((s) => ({
      streamContent: s.streamContent + token,
      isStreaming: true,
    })),
  clearStreamContent: () => set({ streamContent: "", isStreaming: false }),
  addMessage: (msg) => set((s) => ({ messages: [...s.messages, msg] })),
  updateMessage: (id, updates) =>
    set((s) => ({
      messages: s.messages.map((m) =>
        m.id === id ? { ...m, ...updates } : m
      ),
    })),
  loadConversationTurns: async () => {
    if (!isTauriRuntime()) return;
    try {
      const turns = await invoke<ConversationTurn[]>("get_agent_conversation");
      set({ conversationTurns: turns });
    } catch (err) {
      // 读不到上下文不该让面板炸掉：这是一个信息展示，不是运行的一部分
      console.warn("[AgentStore] get_agent_conversation failed:", err);
    }
  },
  truncateConversationFrom: async (turnId) => {
    if (!isTauriRuntime()) return;
    // 后端把切完剩下的几轮一起返回，这里不再查第二次：中间多一次往返就多一个
    // "界面显示的和后端实际的不一致"的窗口。
    const turns = await invoke<ConversationTurn[]>("truncate_agent_conversation", {
      turnId,
    });
    set({ conversationTurns: turns });
  },




  // ====== 权限管理实现 ======
  setPermissionPreset: (preset) => {
    const permissions = permissionsForPreset(preset);
    set({ permissionPreset: preset, permissions });
  },
  togglePermission: (key) =>
    set((s) => ({
      permissions: { ...s.permissions, [key]: !s.permissions[key] },
    })),
  setBrowserOrigins: (origins) =>
    set((s) => ({
      permissions: { ...s.permissions, browserOrigins: origins },
    })),
  setPageReadOrigins: (origins) =>
    set((s) => ({
      permissions: { ...s.permissions, pageReadOrigins: origins },
    })),
  setInputApps: (apps) =>
    set((s) => ({
      permissions: { ...s.permissions, inputApps: apps },
    })),
  setComputerApps: (apps) =>
    set((s) => ({
      permissions: { ...s.permissions, computerApps: apps },
    })),
  setCaptureApps: (apps) =>
    set((s) => ({
      permissions: { ...s.permissions, captureApps: apps },
    })),
  requestConfirm: (confirm) =>
    set((s) => {
      if (s.pendingConfirm && s.pendingConfirm.id !== confirm.id) {
        // 后端可以同时挂多条（registry 是 map），而这里只有一个槽。被顶掉的那条请求
        // 在后端还挂着、用户却再也看不到它，只能白等到超时 —— 至少要留一句，别让
        // "运行卡住了"查不出原因。
        console.warn(
          "[AgentStore] replacing a pending approval request that was never answered:",
          s.pendingConfirm.id
        );
      }
      return { pendingConfirm: confirm };
    }),
  resolveConfirm: async (approved) => {
    const pending = get().pendingConfirm;
    if (!pending) {
      return false;
    }
    // 先收对话框再等后端：等待期间它还开着的话，第二次点击会送第二个决定，
    // 而后端那条请求已经被第一次点击取走了
    set({ pendingConfirm: null });
    if (!isTauriRuntime()) {
      return false;
    }
    try {
      const heard = await invoke<boolean>("resolve_agent_approval", {
        requestId: pending.id,
        approved,
      });
      if (!heard) {
        // 后端已经不等这条请求了（超时，或 Stop 拒过了）。这一次点击什么都没授权，
        // 而对话框已经关掉：只写 console 的话，"点了批准"和"批准生效"在界面上完全
        // 一样，而这个对话框的全部意义就是让用户知道他授权了什么。
        set({
          error: `That approval arrived too late — the action was already refused (${pending.title}).`,
        });
      }
      return heard;
    } catch (err) {
      console.warn("[AgentStore] resolve_agent_approval failed:", err);
      set({ error: `Could not send that approval decision: ${String(err)}` });
      return false;
    }
  },
  closeConfirm: (requestId) =>
    set((s) =>
      // id 要对上：后端关掉的是**那一条**请求，而这时挂着的可能已经是下一条了
      s.pendingConfirm && s.pendingConfirm.id === requestId ? { pendingConfirm: null } : s
    ),

  // ========== 异步 Actions (IPC) ==========
  sendPrompt: async (params) => {
    const runId = makeAgentRunId("chat");
    const requestIdeMode = params.ideMode ?? get().ideMode;
    // 标题在这里定下来：prompt 的第一行。后端没有任务标题的概念，而 Plan 标题和运行
    // 摘要两处都在读它 —— 在这之前它永远是 null，两处都渲染硬编码的字面量。
    const title = deriveTaskTitle(params.prompt);
    set({
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: true,
      agentRunId: runId,
      restoredSession: null,
      // 标题推不出来（整段 prompt 只有空白）时保留上一轮的，而不是把它抹成 null
      currentTask: title ? { id: runId, title } : get().currentTask,
      // 上一轮测到的占用不属于这一轮。不清掉的话，一次没有用量回报的运行（本地
      // runtime、mock，或者中途失败）会让界面继续显示上一轮的数字，而标签写着"这次"。
      contextUsage: null,
    });
    persistAgentSession(get());
    try {
      if (!isTauriRuntime()) {
        throw new Error("Agent backend is available in the Tauri app runtime.");
      }
      await invoke("send_agent_prompt", {
        request: {
          prompt: params.prompt,
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
          contextSources: params.contextSources ?? null,
          ideRuntime: params.ideRuntime ?? null,
          toolApproval: mcpApprovalForPermissions(get().permissions),
          allowFileCreate: get().permissions.allowFileCreate,
          allowBrowserUse: get().permissions.allowBrowserUse,
          browserOrigins: get().permissions.browserOrigins,
          allowPageRead: get().permissions.allowPageRead,
          pageReadOrigins: get().permissions.pageReadOrigins,
          allowComputerUse: get().permissions.allowComputerUse,
          computerApps: get().permissions.computerApps,
          allowComputerCapture: get().permissions.allowComputerCapture,
          captureApps: get().permissions.captureApps,
          allowComputerInput: get().permissions.allowComputerInput,
          inputApps: get().permissions.inputApps,
          allowCommandRun: get().permissions.allowCommandRun,
          runId,
          ideMode: requestIdeMode,
        },
      });
      if (requestIdeMode === "plan" && get().activeSddArtifact) {
        get().addMessage({
          id: `sdd-ready-${Date.now()}`,
          role: "system",
          content: `SDD draft ready: ${get().activeSddArtifact?.title}`,
          timestamp: Date.now(),
        });
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "Agent task cancelled") {
        set({ error: null, state: "idle" });
      } else {
        set({ error: msg, state: "error" });
      }
    } finally {
      set({ isStreaming: false });
    }
  },

  stopAgent: async () => {
    try {
      if (!isTauriRuntime()) {
        set({ state: "idle", steps: [], diffs: [], agentRunId: null, restoredSession: null });
        persistAgentSession(get());
        return;
      }
      await invoke("stop_agent");
      set({ state: "idle", steps: [], diffs: [], agentRunId: null, restoredSession: null });
      persistAgentSession(get());
    } catch (err: unknown) {
      console.warn("[AgentStore] stop_agent failed:", err);
    }
  },

  changeMode: async (mode) => {
    try {
      if (!isTauriRuntime()) {
        set({ mode });
        return;
      }
      await invoke("set_agent_mode", { mode });
      set({ mode });
    } catch (err: unknown) {
      console.warn("[AgentStore] set_agent_mode failed:", err);
      set({ mode });
    }
  },

  applyAllDiffs: async () => {
    const reviewable = get().diffs.filter(isReviewableDiff);
    if (!isTauriRuntime()) {
      set({ error: "Applying diffs needs the desktop runtime (npm run tauri -- dev)." });
      return [];
    }
    try {
      const result = await invoke<ApplyDiffsResult>("apply_diffs");
      // 后端的 diff 只活在内存里，前端却把它们写进了 localStorage。重启之后界面
      // 还显示 N 条待处理，后端手上是空的，apply 就成了静默空操作 —— 按钮点了
      // 没反应、文件没变、也没有任何提示。这里把这种不一致明确报出来。
      if (result.applied.length === 0 && result.failed.length === 0) {
        set({
          lastApplyResult: result,
          error:
            reviewable.length > 0
              ? `Nothing was applied: the backend has no pending diffs for this review list. It was restored from an earlier session, so re-run the task to regenerate the changes.`
              : null,
        });
        return [];
      }
      set((s) => {
        const diffs = s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0
            ? `Failed to apply ${result.failed.length} diff${result.failed.length === 1 ? "" : "s"}.`
            : null,
          diffs,
        };
      });
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diffs failed:", err);
      set({ error: `Apply all diffs failed: ${describeError(err)}` });
      return [];
    }
  },

  undoLastApply: async () => {
    try {
      if (!isTauriRuntime()) return false;
      const result = await invoke<{ label: string; restored: string[]; failed: string[] }>(
        "undo_last_apply",
      );
      // 后端会重发 agent-diff-ready，diff 列表由事件刷新；这里只负责反馈
      set({
        error:
          result.failed.length > 0
            ? `Undo restored ${result.restored.length} file(s); ${result.failed.length} could not be restored.`
            : null,
      });
      await get().refreshPendingUndo();
      return result.failed.length === 0;
    } catch (err: unknown) {
      // "没有可撤销的操作"也走这里，作为提示展示出来是合理反馈
      set({ error: err instanceof Error ? err.message : String(err) });
      await get().refreshPendingUndo();
      return false;
    }
  },

  setPendingUndo: (pendingUndo) => set({ pendingUndo }),
  setRunUsage: (runUsage) => set({ runUsage }),

  refreshPendingUndo: async () => {
    if (!isTauriRuntime()) return;
    try {
      const pending = await invoke<{ label: string; files: string[] } | null>("pending_undo");
      set({ pendingUndo: pending ?? null });
    } catch (err: unknown) {
      // 查询失败不该覆盖 error 横幅：这是背景刷新，不是用户发起的动作。
      // 保守起见按"没有退路"处理，宁可不显示按钮，也不显示一个点不动的按钮。
      console.warn("[AgentStore] pending_undo failed:", err);
      set({ pendingUndo: null });
    }
  },


  applyDiff: async (diffId) => {
    try {
      if (!isTauriRuntime()) return [];
      const result = await invoke<ApplyDiffsResult>("apply_diff", { diffId });
      set((s) => {
        const diffs = s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0 ? "Failed to apply diff." : null,
          diffs,
        };
      });
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diff failed:", err);
      return [];
    }
  },

  applyDiffHunk: async (diffId, hunkIndex) => {
    try {
      if (!isTauriRuntime()) return [];
      const result = await invoke<ApplyDiffsResult>("apply_diff_hunk", { diffId, hunkIndex });
      set((s) => {
        const diffs = s.diffs.map((d) => {
          if (d.id !== diffId) return d;
          const failed = result.failed.find((failure) => failure.diffId === d.id);
          const applied = result.applied.some((item) => item.id === d.id);
          const hunks = d.hunks.map((hunk, index) =>
            index === hunkIndex
              ? {
                  ...hunk,
                  status: applied ? "applied" as const : failed ? "failed" as const : hunk.status,
                  applyError: failed?.message,
                }
              : hunk
          );
          return {
            ...d,
            hunks,
            status: nextDiffStatus(hunks),
            applyError: failed?.message,
          };
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0 ? "Failed to apply hunk." : null,
          diffs,
        };
      });
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diff_hunk failed:", err);
      return [];
    }
  },

  rejectAllDiffs: async () => {
    const reviewable = get().diffs.filter(isReviewableDiff);
    if (!isTauriRuntime()) {
      set({ error: "Rejecting diffs needs the desktop runtime (npm run tauri -- dev)." });
      return [];
    }
    try {
      const rejected = await invoke<DiffEntry[]>("reject_diffs");
      if (rejected.length === 0) {
        set({
          lastApplyResult: null,
          error:
            reviewable.length > 0
              ? `Nothing was rejected: the backend has no pending diffs for this review list. It was restored from an earlier session, so re-run the task to regenerate the changes.`
              : null,
        });
        return [];
      }
      set((s) => {
        const diffs = s.diffs.map((d) =>
          rejected.some((r) => r.id === d.id)
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        );
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diffs failed:", err);
      set({ error: `Reject all diffs failed: ${describeError(err)}` });
      return [];
    }
  },

  rejectDiff: async (diffId) => {
    try {
      if (!isTauriRuntime()) return null;
      const rejected = await invoke<DiffEntry>("reject_diff", { diffId });
      set((s) => {
        const diffs = s.diffs.map((d) =>
          d.id === rejected.id
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        );
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diff failed:", err);
      return null;
    }
  },

  rejectDiffHunk: async (diffId, hunkIndex) => {
    try {
      if (!isTauriRuntime()) return null;
      const rejected = await invoke<DiffEntry>("reject_diff_hunk", { diffId, hunkIndex });
      set((s) => {
        const diffs = s.diffs.map((d) => (d.id === rejected.id ? rejected : d));
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diff_hunk failed:", err);
      return null;
    }
  },

  estimateContext: async (params) => {
    try {
      if (!isTauriRuntime()) return null;
      return await invoke<ContextEstimateResponse>("estimate_agent_context", {
        request: {
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
          contextSources: params.contextSources ?? null,
          ideRuntime: params.ideRuntime ?? null,
        },
      });
    } catch (err: unknown) {
      console.warn("[AgentStore] estimate_agent_context failed:", err);
      return null;
    }
  },

  updateAgentStep: async (step) => {
    try {
      if (!isTauriRuntime()) {
        get().updateStep(step.id, step);
        return step;
      }
      const updated = await invoke<Step>("update_agent_step", { step });
      get().updateStep(updated.id, updated);
      return updated;
    } catch (err: unknown) {
      console.warn("[AgentStore] update_agent_step failed:", err);
      return null;
    }
  },

  updateAgentSteps: async (steps) => {
    try {
      if (!isTauriRuntime()) {
        set({ steps });
        persistAgentSession({ ...get(), steps });
        return steps;
      }
      const updated = await invoke<Step[]>("update_agent_steps", { steps });
      set({ steps: updated });
      persistAgentSession({ ...get(), steps: updated });
      return updated;
    } catch (err: unknown) {
      console.warn("[AgentStore] update_agent_steps failed:", err);
      set({ steps });
      persistAgentSession({ ...get(), steps });
      return steps;
    }
  },

  skipAgentStep: async (stepId) => {
    try {
      if (!isTauriRuntime()) {
        get().updateStep(stepId, { status: "skipped" });
        return get().steps.find((step) => step.id === stepId) ?? null;
      }
      const updated = await invoke<Step>("skip_agent_step", { stepId });
      get().updateStep(updated.id, updated);
      return updated;
    } catch (err: unknown) {
      console.warn("[AgentStore] skip_agent_step failed:", err);
      return null;
    }
  },

  runAgentStep: async (params) => {
    const runId = makeAgentRunId("step");
    set({
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: true,
      agentRunId: runId,
      restoredSession: null,
      // 见 `sendPrompt`：上一轮测到的占用不属于这一轮
      contextUsage: null,
    });
    persistAgentSession(get());
    try {
      if (!isTauriRuntime()) {
        throw new Error("Agent backend is available in the Tauri app runtime.");
      }
      await invoke("run_agent_step", {
        request: {
          step: params.step,
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
          contextSources: params.contextSources ?? null,
          toolApproval: mcpApprovalForPermissions(get().permissions),
          allowCommandRun: get().permissions.allowCommandRun,
          allowFileCreate: get().permissions.allowFileCreate,
          allowBrowserUse: get().permissions.allowBrowserUse,
          browserOrigins: get().permissions.browserOrigins,
          allowPageRead: get().permissions.allowPageRead,
          pageReadOrigins: get().permissions.pageReadOrigins,
          allowComputerUse: get().permissions.allowComputerUse,
          computerApps: get().permissions.computerApps,
          allowComputerCapture: get().permissions.allowComputerCapture,
          captureApps: get().permissions.captureApps,
          allowComputerInput: get().permissions.allowComputerInput,
          inputApps: get().permissions.inputApps,
          extraPrompt: params.extraPrompt ?? null,
          regeneratedFromDiffId: params.regeneratedFromDiffId ?? null,
          regeneratedFromHunkIndex: params.regeneratedFromHunkIndex ?? null,
          runId,
        },
      });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "Agent task cancelled") {
        set({ error: null, state: "idle" });
      } else {
        set({ error: msg, state: "error" });
      }
    } finally {
      set({ isStreaming: false });
    }
  },

  continueAgentPipeline: async () => {
    set({ error: null, streamContent: "", isStreaming: true, restoredSession: null, contextUsage: null });
    persistAgentSession(get());
    try {
      if (!isTauriRuntime()) {
        throw new Error("Agent backend is available in the Tauri app runtime.");
      }
      await invoke("continue_agent_pipeline");
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "Agent task cancelled") {
        set({ error: null, state: "idle" });
      } else {
        set({ error: msg, state: "error" });
      }
    } finally {
      set({ isStreaming: false });
      persistAgentSession(get());
    }
  },

  regenerateDiff: async (params) => {
    const hunk = params.hunkIndex != null ? params.diff.hunks[params.hunkIndex] : undefined;
    const targetHunks = hunk ? [hunk] : params.diff.hunks;
    const prompt = [
      "Regenerate this Agent IDE diff against the current file content.",
      "Keep the original intent, but make the replacement hunks match the file as it exists now.",
      "Return reviewable Agent IDE diffs for this file only.",
      "",
      `File: ${params.diff.file}`,
      `Failed diff id: ${params.diff.id}`,
      params.hunkIndex != null ? `Failed hunk index: ${params.hunkIndex}` : null,
      params.diff.applyError ? `Apply error: ${params.diff.applyError}` : null,
      params.diff.provenance ? `Original provenance: ${JSON.stringify(params.diff.provenance)}` : null,
      "",
      "Original generated hunks:",
      JSON.stringify(targetHunks, null, 2),
      "",
      "Current file content:",
      "```",
      params.currentFileContent ?? params.activeFileContent ?? "",
      "```",
    ].filter(Boolean).join("\n");

    await get().runAgentStep({
      step: {
        id: `regen-${params.diff.id}-${params.hunkIndex ?? "file"}-${Date.now()}`,
        title: `Regenerate ${params.diff.file}`,
        type: "edit",
        status: "todo",
        logs: [],
        scope: "active_file",
        executionMode: "fix",
      },
      contextFiles: params.contextFiles,
      activeFile: params.activeFile ?? params.diff.file,
      activeFileContent: params.currentFileContent ?? params.activeFileContent,
      selection: params.selection,
      profileId: params.profileId,
      contextCompression: params.contextCompression,
      contextSources: params.contextSources,
      extraPrompt: prompt,
      regeneratedFromDiffId: params.diff.id,
      regeneratedFromHunkIndex: params.hunkIndex,
    });
  },

  // ========== 模型配置 ==========
  fetchLlmConfig: async () => {
    try {
      if (!isTauriRuntime()) {
        set({ llmConfigured: false });
        return;
      }
      const cfg = await invoke<LlmConfigResponse>("get_llm_config");
      set({
        llmConfigured: true,
        llmEndpoint: cfg.endpoint,
        llmModel: cfg.model,
        apiKeyMasked: cfg.api_key_masked,
        contextCompression: cfg.context_compression,
        llmProfiles: cfg.profiles ?? [],
        activeProfileId: cfg.active_profile_id ?? "",
        chatProfileId: resolveChatProfileId(
          get().chatProfileId,
          cfg.profiles ?? [],
          cfg.active_profile_id ?? null
        ),
      });
    } catch {
      set({ llmConfigured: false });
    }
  },

  saveLlmProfile: async (request) => {
    if (!isTauriRuntime()) {
      throw new Error("LLM profile management is available in the Tauri app runtime.");
    }
    const response = await invoke<LlmProfilesResponse>("save_llm_profile", { request });
    applyProfilesResponse(response, set, get);
  },

  deleteLlmProfile: async (profileId) => {
    if (!isTauriRuntime()) {
      throw new Error("LLM profile management is available in the Tauri app runtime.");
    }
    const response = await invoke<LlmProfilesResponse>("delete_llm_profile", { profileId });
    applyProfilesResponse(response, set, get);
  },

  setActiveLlmProfile: async (profileId) => {
    if (!isTauriRuntime()) {
      set({ activeProfileId: profileId, chatProfileId: profileId });
      return;
    }
    const response = await invoke<LlmProfilesResponse>("set_active_llm_profile", { profileId });
    applyProfilesResponse(response, set, get);
  },

  setChatProfileId: (profileId) => set({ chatProfileId: profileId }),
  setChatContextCompression: (mode) => set({ chatContextCompression: mode }),

  /** 显式取一次明文密钥，只在用户点击"显示"时调用 */
  revealLlmApiKey: async (profileId) => {
    if (!isTauriRuntime()) {
      throw new Error("Reading stored secrets requires the Tauri app runtime.");
    }
    return invoke<string>("reveal_llm_api_key", { profileId: profileId ?? null });
  },


  updateContextCompression: async (mode) => {
    if (!isTauriRuntime()) {
      set({ contextCompression: mode });
      return;
    }
    const saved = await invoke<ContextCompressionMode>("set_context_compression", { mode });
    set({ contextCompression: saved });
  },

  // ========== 角色管理 ==========
  setActiveRole: async (role) => {
    if (!isTauriRuntime()) {
      set({ activeRole: role });
      return;
    }
    await invoke("set_active_role", { role });
    set({ activeRole: role });
  },

  fetchActiveRole: async () => {
    try {
      if (!isTauriRuntime()) return;
      const role = await invoke<string>("get_active_role");
      set({ activeRole: role as AgentRole });
    } catch {
      // keep default
    }
  },

  // ========== 流水线管理 ==========
  fetchPipeline: async () => {
    try {
      if (!isTauriRuntime()) return;
      const stages = await invoke<PipelineStage[]>("get_pipeline");
      set({ pipeline: stages });
    } catch {
      // keep default
    }
  },

  updatePipeline: async (stages) => {
    if (!isTauriRuntime()) {
      set({ pipeline: stages });
      return;
    }
    await invoke("update_pipeline", { stages });
    set({ pipeline: stages });
  },

  resetPipeline: async () => {
    if (!isTauriRuntime()) {
      return;
    }
    const stages = await invoke<PipelineStage[]>("reset_pipeline");
    set({ pipeline: stages });
  },

  // ========== 连通性测试 ==========
  testLlmConnection: async () => {
    if (!isTauriRuntime()) {
      throw new Error("LLM connection test is available in the Tauri app runtime.");
    }
    const target = llmTargetFingerprint(get());
    try {
      const result = await invoke<string>("test_llm_connection", {
        profileId: get().chatProfileId,
      });
      set({ llmConnection: { status: "ok", checkedAt: Date.now(), detail: result, target } });
      return result;
    } catch (error) {
      // 失败也要记下来。只往上抛的话，调用方把它变成一句转瞬即逝的提示，状态栏
      // 那个点继续说"ready" —— 而它其实只知道"配置过"。
      set({
        llmConnection: {
          status: "failed",
          checkedAt: Date.now(),
          detail: String(error),
          target,
        },
      });
      throw error;
    }
  },
}));

/**
 * chat 用哪个 profile：保留用户的选择，但选择必须还存在。
 *
 * 后端拿到一个不认识的 id 时会**静默退回列表里的第一个**
 * （`llm_profiles.rs` 的 `.or_else(|| config.profiles.first())`）。所以删掉正在用的
 * profile 之后，如果还留着那个死 id，前端说的是一个目标，实际发出去的是另一个 ——
 * 连通性和每一次 prompt 都会算错账。
 */
function resolveChatProfileId(
  current: string | null,
  profiles: LlmProfile[],
  activeProfileId: string | null
): string | null {
  if (current && profiles.some((profile) => profile.id === current)) {
    return current;
  }
  return activeProfileId ?? profiles[0]?.id ?? null;
}

function applyProfilesResponse(
  response: LlmProfilesResponse,
  set: (partial: Partial<AgentStore>) => void,
  get: () => AgentStore
) {
  const active =
    response.profiles.find((profile) => profile.id === response.active_profile_id) ??
    response.profiles[0];
  set({
    llmConfigured: response.profiles.length > 0,
    llmProfiles: response.profiles,
    activeProfileId: response.active_profile_id,
    chatProfileId: resolveChatProfileId(
      get().chatProfileId,
      response.profiles,
      response.active_profile_id
    ),
    contextCompression: response.context_compression,
    llmEndpoint: active?.endpoint ?? "",
    llmModel: active?.model ?? "",
    apiKeyMasked: active?.api_key_masked ?? "",
  });
}

function nextDiffStatus(hunks: DiffEntry["hunks"]): DiffEntry["status"] {
  if (hunks.length > 0 && hunks.every((hunk) => hunk.status === "applied")) {
    return "applied";
  }
  if (hunks.length > 0 && hunks.every((hunk) => hunk.status === "rejected")) {
    return "rejected";
  }
  if (hunks.some((hunk) => hunk.status === "failed")) {
    return "failed";
  }
  if (hunks.some((hunk) => hunk.status === "applied" || hunk.status === "rejected")) {
    return "partial";
  }
  return "pending";
}

const AGENT_DIFFS_STORAGE_KEY = "agent-ide-agent-diffs";
const AGENT_SESSION_STORAGE_KEY = "agent-ide-agent-session";

/// 对应后端 `is_reviewable_diff_status`：还能被 Apply/Reject 处理的状态。
const REVIEWABLE_DIFF_STATUSES = new Set(["pending", "partial", "failed"]);

export function isReviewableDiff(diff: DiffEntry): boolean {
  return REVIEWABLE_DIFF_STATUSES.has(diff.status);
}

function describeError(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  return JSON.stringify(err);
}

interface PersistedAgentSession {
  workspacePath: string;
  runId: string | null;

  state: AgentState;
  mode: AgentMode;
  ideMode: IdeMode;
  currentTask: Task | null;
  steps: Step[];
  pipeline: PipelineStage[];
  sddArtifacts: SddArtifact[];
  activeSddArtifact: SddArtifact | null;
  error: string | null;
  updatedAt: number;
}

function persistDiffs(diffs: DiffEntry[]) {
  if (typeof window === "undefined") return;
  const workspacePath = currentWorkspacePath();
  const payload = {
    workspacePath,
    diffs: diffs.slice(-200),
  };
  localStorage.setItem(AGENT_DIFFS_STORAGE_KEY, JSON.stringify(payload));
}

function loadDiffs(expectedWorkspacePath = currentWorkspacePath()): DiffEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = localStorage.getItem(AGENT_DIFFS_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { workspacePath?: string; diffs?: DiffEntry[] };
    if (!Array.isArray(parsed.diffs)) return [];
    if (expectedWorkspacePath && parsed.workspacePath && parsed.workspacePath !== expectedWorkspacePath) {
      return [];
    }
    return parsed.diffs.slice(-200);
  } catch {
    return [];
  }
}

function persistAgentSession(state: Pick<AgentStore, "state" | "mode" | "ideMode" | "currentTask" | "steps" | "pipeline" | "sddArtifacts" | "activeSddArtifact" | "error" | "agentRunId">) {
  if (typeof window === "undefined") return;
  const workspacePath = currentWorkspacePath();
  const hasSessionData = state.steps.length > 0 || state.pipeline.some((stage) => stage.status !== "pending") || state.currentTask !== null || Boolean(state.activeSddArtifact);
  if (!workspacePath || !hasSessionData) {
    clearPersistedAgentSession();
    return;
  }
  const payload: PersistedAgentSession = {
    workspacePath,
    runId: state.agentRunId,
    state: normalizeRestoredAgentState(state.state),
    mode: state.mode,
    ideMode: state.ideMode,
    currentTask: state.currentTask,
    steps: state.steps.slice(-100),
    pipeline: state.pipeline,
    sddArtifacts: state.sddArtifacts.slice(-50),
    activeSddArtifact: state.activeSddArtifact,
    error: state.error,
    updatedAt: Date.now(),
  };
  localStorage.setItem(AGENT_SESSION_STORAGE_KEY, JSON.stringify(payload));
}

function loadAgentSession(expectedWorkspacePath = currentWorkspacePath()): Partial<AgentStore> | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(AGENT_SESSION_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<PersistedAgentSession>;
    if (expectedWorkspacePath && parsed.workspacePath && parsed.workspacePath !== expectedWorkspacePath) {
      return null;
    }
    const steps = Array.isArray(parsed.steps) ? parsed.steps : [];
    const pipeline = Array.isArray(parsed.pipeline) && parsed.pipeline.length > 0
      ? parsed.pipeline
      : DEFAULT_PIPELINE;
    // 任务先归一化再判断"这份会话还有内容吗"：判断和恢复用同一个值，否则一份
    // 形状不对的 currentTask 能让一个空会话复活 —— 恢复出来的 store 里它已经是 null。
    const currentTask = normalizeRestoredTask(parsed.currentTask);
    if (steps.length === 0 && pipeline.every((stage) => stage.status === "pending") && !currentTask) {
      return null;
    }
    const interrupted = isInFlightState(parsed.state);
    return {
      state: normalizeRestoredAgentState(parsed.state),
      mode: normalizeAgentMode(parsed.mode),
      ideMode: parsed.ideMode ?? "code",
      currentTask,
      steps: steps.map(normalizeRestoredStep),
      pipeline: pipeline.map(normalizeRestoredPipelineStage),
      sddArtifacts: Array.isArray(parsed.sddArtifacts) ? parsed.sddArtifacts : [],
      activeSddArtifact: parsed.activeSddArtifact ?? null,
      error: parsed.error ?? null,
      agentRunId: parsed.runId ?? null,
      restoredSession: {
        runId: parsed.runId ?? null,
        restoredAt: Date.now(),
        interrupted,
        backendMatched: null,
        updatedAt: parsed.updatedAt,
      },
      streamContent: "",
      isStreaming: false,
    };
  } catch {
    return null;
  }
}

function clearPersistedAgentSession() {
  if (typeof window === "undefined") return;
  localStorage.removeItem(AGENT_SESSION_STORAGE_KEY);
}

function normalizeRestoredAgentState(state?: AgentState): AgentState {
  if (state === "thinking" || state === "planning" || state === "acting" || state === "reviewing") {
    return "waiting_user";
  }
  return state ?? "idle";
}

function isInFlightState(state?: AgentState): boolean {
  return state === "thinking" || state === "planning" || state === "acting" || state === "reviewing";
}

/**
 * localStorage 里的任务只认两个字符串字段。
 *
 * 旧版本存进去的 `Task` 还带 `status` / `steps` / `affectedFiles`，直接铺回 store
 * 会让一个早已删掉的形状重新出现在状态里；而标题是要渲染的，类型不对就会在
 * 标题栏里出现 `[object Object]`。
 */
function normalizeRestoredTask(task: unknown): Task | null {
  if (!task || typeof task !== "object") return null;
  const { id, title } = task as Record<string, unknown>;
  if (typeof id !== "string" || typeof title !== "string" || title.trim() === "") {
    return null;
  }
  return { id, title };
}

function normalizeRestoredStep(step: Step): Step {
  if (step.status === "doing") {
    return {
      ...step,
      status: "error",
      logs: [...(step.logs ?? []), "Interrupted by reload before completion."],
    };
  }
  return { ...step, logs: step.logs ?? [] };
}

function normalizeRestoredPipelineStage(stage: PipelineStage): PipelineStage {
  return stage.status === "active" ? { ...stage, status: "failed" } : stage;
}

function windowQueuePersist(callback: () => void) {
  if (typeof window === "undefined") return;
  window.queueMicrotask(callback);
}

function makeAgentRunId(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function currentWorkspacePath() {
  try {
    return localStorage.getItem("agent-ide-workspace-path") ?? "";
  } catch {
    return "";
  }
}
