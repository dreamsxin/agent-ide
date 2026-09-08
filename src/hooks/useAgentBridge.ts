import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAgentStore } from "../stores/useAgentStore";
import { useLogStore } from "../stores/useLogStore";
import { useProblemStore } from "../stores/useProblemStore";
import type { AgentState, Step, DiffEntry, PipelineStage, AgentActionLogEntry, SddArtifact } from "../types/agent";
import { isTauriRuntime } from "../utils/tauri";

interface StateChangedPayload {
  state: string;
  mode?: string;
  ideMode?: string;
  currentRunId?: string | null;
  lastRunId?: string | null;
}

/**
 * Agent Bridge: 监听 Tauri 后端事件并同步到 Zustand store
 * 在 App 顶层挂载一次即可
 */
export function useAgentBridge() {
  const setState = useAgentStore((s) => s.setState);
  const setSteps = useAgentStore((s) => s.setSteps);
  const updateStep = useAgentStore((s) => s.updateStep);
  const setDiffs = useAgentStore((s) => s.setDiffs);
  const setSddArtifact = useAgentStore((s) => s.setSddArtifact);
  const setPipeline = useAgentStore((s) => s.setPipeline);
  const appendStreamContent = useAgentStore((s) => s.appendStreamContent);
  const clearStreamContent = useAgentStore((s) => s.clearStreamContent);
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
              useAgentStore.getState().setMode(mode as "suggest" | "edit" | "auto");
            }
            if (e.payload.ideMode) {
              useAgentStore.getState().setIdeMode(e.payload.ideMode as "code" | "plan");
            }
            // 每条应用路径（Apply All / 单文件 / 单 hunk / 自动应用 / 撤销）都会发这个
            // 事件，而 `apply_diffs` 并不重发 agent-diff-ready —— 所以"有没有退路"
            // 必须在这里也复查一次，否则手动 Apply All 之后 Undo 按钮不会出现。
            void useAgentStore.getState().refreshPendingUndo();
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
            // 应用、自动应用、Agent 工具写文件都会重发这个事件，所以这里是
            // "有没有可撤销的应用"唯一需要复查的地方。
            void useAgentStore.getState().refreshPendingUndo();
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
            setPipeline(e.payload);
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
  }, [addLog, appendStreamContent, clearStreamContent, setDiffs, setPipeline, setSddArtifact, setState, setSteps, updateStep, upsertProblems]);
}

function formatLogTime(timestamp: string) {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) {
    return new Date().toLocaleTimeString();
  }
  return parsed.toLocaleTimeString();
}
