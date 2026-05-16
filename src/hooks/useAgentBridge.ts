import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAgentStore } from "../stores/useAgentStore";
import { useLogStore } from "../stores/useLogStore";
import { useProblemStore } from "../stores/useProblemStore";
import type { AgentState, Step, DiffEntry, PipelineStage, AgentActionLogEntry } from "../types/agent";
import { isTauriRuntime } from "../utils/tauri";

interface StateChangedPayload {
  state: string;
  mode?: string;
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
  }, [addLog, appendStreamContent, clearStreamContent, setDiffs, setPipeline, setState, setSteps, updateStep, upsertProblems]);
}

function formatLogTime(timestamp: string) {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) {
    return new Date().toLocaleTimeString();
  }
  return parsed.toLocaleTimeString();
}
