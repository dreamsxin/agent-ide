import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAgentStore } from "../stores/useAgentStore";
import type { AgentState, Step, DiffEntry } from "../types/agent";

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
  const appendStreamContent = useAgentStore((s) => s.appendStreamContent);
  const clearStreamContent = useAgentStore((s) => s.clearStreamContent);

  useEffect(() => {
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
            updateStep(step.id, { status: step.status, logs: step.logs });
          }),

          listen<DiffEntry[]>("agent-diff-ready", (e) => {
            setDiffs(e.payload);
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
  }, []);
}
