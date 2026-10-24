import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAgentStore } from "../stores/useAgentStore";
import { useLogStore } from "../stores/useLogStore";
import { useProblemStore } from "../stores/useProblemStore";
import type { AgentState, Step, DiffEntry, PipelineStage, AgentActionLogEntry, SddArtifact } from "../types/agent";
import { normalizeAgentMode, normalizeAgentQuestion, normalizeApprovalRequest, normalizeContextUsage, normalizeRunUsage } from "../types/agent";
import { isTauriRuntime } from "../utils/tauri";

interface StateChangedPayload {
  state: string;
  mode?: string;
  ideMode?: string;
  currentRunId?: string | null;
  lastRunId?: string | null;
  /** 当前可撤销的那次应用，`null` 表示没有退路。由后端在锁内计算，见 state_payload */
  pendingUndo?: { label: string; files: string[] } | null;
  /** 本次运行至今的 token/花费，同样由 state_payload 在锁内算出 */
  usage?: unknown;
}

/**
 * Agent Bridge: 监听 Tauri 后端事件并同步到 Zustand store
 * 在 App 顶层挂载一次即可
 */
export function useAgentBridge() {
  const setState = useAgentStore((s) => s.setState);
  const setSteps = useAgentStore((s) => s.setSteps);
  const setContextUsage = useAgentStore((s) => s.setContextUsage);
  const updateStep = useAgentStore((s) => s.updateStep);
  const setDiffs = useAgentStore((s) => s.setDiffs);
  const setSddArtifact = useAgentStore((s) => s.setSddArtifact);
  const setRunPipeline = useAgentStore((s) => s.setRunPipeline);
  const appendStreamContent = useAgentStore((s) => s.appendStreamContent);
  const clearStreamContent = useAgentStore((s) => s.clearStreamContent);
  const requestConfirm = useAgentStore((s) => s.requestConfirm);
  const closeConfirm = useAgentStore((s) => s.closeConfirm);
  const requestQuestion = useAgentStore((s) => s.requestQuestion);
  const closeQuestion = useAgentStore((s) => s.closeQuestion);
  const addLog = useLogStore((s) => s.addLog);
  const upsertProblems = useProblemStore((s) => s.upsertProblems);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let stopped = false;
    const unlisteners: Array<() => void> = [];

    // 用 async IIFE 收集所有异步 listen，确保 StrictMode 下正确清理
    (async () => {
      try {
        const fns = await Promise.all([
          listen<StateChangedPayload>("agent-state-changed", (e) => {
            const { state, mode } = e.payload;
            setState(state as AgentState);
            if (mode) {
              useAgentStore.getState().setMode(normalizeAgentMode(mode));
            }
            if (e.payload.ideMode) {
              useAgentStore.getState().setIdeMode(e.payload.ideMode as "code" | "plan");
            }
            // 撤销可用性跟在这个事件的 payload 里，不再回头去 invoke 查询：查询要抢
            // orchestrator 锁，而运行期间那把锁被整条流水线占着。payload 由刚改完
            // 撤销栈的同一段代码在同一个临界区里算出，既新鲜也不可能漂移。
            useAgentStore.getState().setPendingUndo(e.payload.pendingUndo ?? null);
            // 用量搭同一趟车。走 normalize 而不是 `as RunUsage`：这是事件数据，
            // 硬转只是让类型检查闭嘴，旧版本后端少一个字段就会以 NaN 渲染出来。
            useAgentStore.getState().setRunUsage(normalizeRunUsage(e.payload.usage));
          }),

          listen<Step[]>("agent-plan-ready", (e) => {
            setSteps(e.payload);
            clearStreamContent();
          }),

          listen<Step>("agent-step-update", (e) => {
            const step = e.payload;
            updateStep(step.id, step);
          }),

          listen<DiffEntry[]>("agent-diff-ready", (e) => {
            // 不再强制切到 Changes：待审查改动现在直接出现在对话流里
            // （PendingChangesCard），把用户从刚读的回复上拽走反而更差。
            setDiffs(e.payload);
            upsertProblems(
              "agent",
              e.payload
                .filter((diff) => diff.status === "failed")
                .map((diff) => ({
                  id: `agent-diff-${diff.id}`,
                  file: diff.file,
                  line: diff.hunks[0]?.oldStart || diff.hunks[0]?.newStart || 1,
                  column: 1,
                  severity: "error",
                  source: "agent",
                  message: diff.applyError ?? "Agent diff failed to apply",
                }))
            );
          }),

          listen<SddArtifact>("agent-sdd-ready", (e) => {
            setSddArtifact(e.payload);
            clearStreamContent();
          }),

          listen<PipelineStage[]>("agent-pipeline-update", (e) => {
            setRunPipeline(e.payload);
          }),

          // 上下文占用的测量值。归一化而不是直接铺进 store：载荷来自事件，见
          // `normalizeContextUsage`（0 代表"没测到"，不是"上下文是空的"）。
          listen<unknown>("agent-context-usage", (e) => {
            setContextUsage(normalizeContextUsage(e.payload));
          }),


          listen<AgentActionLogEntry>("agent-action-log", (e) => {
            const entry = e.payload;
            addLog({
              time: formatLogTime(entry.timestamp),
              level: entry.level,
              source: "agent",
              message: entry.summary,
              details: entry.details,
              phase: entry.phase,
              role: entry.role ?? null,
              stage: entry.stage ?? null,
              contextSummary: entry.contextSummary ?? null,
              diffSummary: entry.diffSummary ?? null,
            });
            // 外部动作的那条日志到了，说明后端刚登记了新的撤不回动作；把审查区那份
            // 拉一次，否则它要等到下次刷新才出现。
            if (entry.phase === "external_action") {
              void useAgentStore.getState().refreshExternalActions();
            }
            if (entry.level === "error") {
              upsertProblems("agent", [
                {
                  id: `agent-log-${entry.id}`,
                  file: entry.stage ?? "Agent",
                  line: 1,
                  column: 1,
                  severity: "error",
                  source: "agent",
                  message: entry.summary,
                },
              ]);
            }
          }),

          listen<string>("agent-stream-token", (e) => {
            appendStreamContent(e.payload);
          }),

          // 一次撤不回的动作正挂在后端等人点。载荷走 normalize：说不清"将要发生
          // 什么"的请求宁可不显示，也不能补个默认值让用户为看不见的事签字。
          listen<unknown>("agent-approval-requested", (e) => {
            const request = normalizeApprovalRequest(e.payload);
            if (request) {
              requestConfirm(request);
              return;
            }
            // 读不懂就**立刻回拒**，而不是丢掉了事：丢掉的话后端要白等满两分钟，
            // 界面表现成"Agent 卡住了"，而真正的原因是这个前端还不认识那种动作。
            console.warn("[useAgentBridge] unreadable approval request, refusing:", e.payload);
            const id = (e.payload as { id?: unknown } | null)?.id;
            if (typeof id === "string" && id) {
              void invoke("resolve_agent_approval", { requestId: id, approved: false }).catch(
                (err) => console.warn("[useAgentBridge] could not refuse it either:", err)
              );
            }
          }),

          // 模型问了一道选择题，正挂在后端等答案。读不懂同样要立刻回一个"没答案"，
          // 否则后端白等两分钟而界面只表现成"Agent 卡住了"。
          listen<unknown>("agent-question-requested", (e) => {
            const question = normalizeAgentQuestion(e.payload);
            if (question) {
              requestQuestion(question);
              return;
            }
            console.warn("[useAgentBridge] unreadable question, declining:", e.payload);
            const id = (e.payload as { id?: unknown } | null)?.id;
            if (typeof id === "string" && id) {
              void invoke("resolve_agent_approval", { requestId: id, approved: false }).catch(
                (err) => console.warn("[useAgentBridge] could not decline it either:", err)
              );
            }
          }),

          // 后端已经不等了（超时或 Stop）。少了这条，超时之后对话框还开着，用户点
          // "批准"却没有任何东西在等他 —— 界面会让他以为自己授权了一次导航。
          // 批准和提问共用这个事件（同一张登记表、同一个 id），两边各按 id 对号。
          listen<{ id?: string }>("agent-approval-closed", (e) => {
            const id = e.payload?.id;
            if (typeof id === "string") {
              closeConfirm(id);
              closeQuestion(id);
            }
          }),
        ]);
        if (!stopped) {
          unlisteners.push(...fns);
        } else {
          // 已被清理 → 立即取消刚注册的 listener
          fns.forEach((fn) => fn());
        }
      } catch (e) {
        console.warn("[useAgentBridge] listen failed:", e);
      }
    })();

    return () => {
      stopped = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [
    addLog,
    appendStreamContent,
    clearStreamContent,
    closeConfirm,
    closeQuestion,
    requestConfirm,
    requestQuestion,
    setContextUsage,
    setDiffs,
    setRunPipeline,
    setSddArtifact,
    setState,
    setSteps,
    updateStep,
    upsertProblems,
  ]);
}

function formatLogTime(timestamp: string) {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) {
    return new Date().toLocaleTimeString();
  }
  return parsed.toLocaleTimeString();
}
