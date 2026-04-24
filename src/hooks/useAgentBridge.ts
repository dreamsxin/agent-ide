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
    const unlisteners: Array<() => void> = [];

    // 监听 Agent 状态变化
    listen<StateChangedPayload>("agent-state-changed", (e) => {
      const { state, mode } = e.payload;
      setState(state as AgentState);
      if (mode) {
        useAgentStore.getState().setMode(mode as "suggest" | "edit" | "auto");
      }
    })
      .then((fn) => unlisteners.push(fn))
      .catch(console.warn);

    // 监听 Plan 就绪
    listen<Step[]>("agent-plan-ready", (e) => {
      setSteps(e.payload);
      clearStreamContent();
    })
      .then((fn) => unlisteners.push(fn))
      .catch(console.warn);

    // 监听单步更新
    listen<Step>("agent-step-update", (e) => {
      const step = e.payload;
      updateStep(step.id, { status: step.status, logs: step.logs });
    })
      .then((fn) => unlisteners.push(fn))
      .catch(console.warn);

    // 监听 Diff 就绪
    listen<DiffEntry[]>("agent-diff-ready", (e) => {
      setDiffs(e.payload);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(console.warn);

    // 监听流式 token
    listen<string>("agent-stream-token", (e) => {
      appendStreamContent(e.payload);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(console.warn);

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);
}
