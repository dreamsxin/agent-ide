import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLayoutStore } from "../stores/useLayoutStore";
import { useEditorStore } from "../stores/useEditorStore";
import { useLogStore } from "../stores/useLogStore";
import { useAgentStore } from "../stores/useAgentStore";
import { isTauriRuntime } from "../utils/tauri";

/**
 * 启动时要做的两件事：加载全局配置、恢复上次的工作区。
 *
 * 从 `App.tsx` 抽出来是为了能测。这类"挂载时机"缺陷不会被 store 层测试看到 ——
 * `fetchLlmConfig` 本身一直是好的，坏的是没人在启动时调它，结果 TopBar 指示灯
 * 一直报 "LLM Not Configured"，直到用户打开一次 Agent 设置面板。渲染整个 `App`
 * 来验证这一点要 mock Monaco、xterm 和一堆懒加载面板，代价和脆弱度都不划算；
 * 一个 hook 只需要 mock store 和 `invoke`。
 */
export function useAppBootstrap() {
  // LLM 配置和工作区无关，所以独立成一个 effect：首次启动、还没有任何工作区时
  // 指示灯照样必须是准的。
  useEffect(() => {
    void useAgentStore.getState().fetchLlmConfig();
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    invoke<string | null>("get_workspace_path")
      .then((saved) => {
        if (saved && typeof saved === "string" && saved.length > 0) {
          console.log("[App] Restoring workspace:", saved);
          useLayoutStore.getState().setWorkspacePath(saved);
          useEditorStore.getState().setWorkspacePath(saved);
          useLogStore.getState().restoreLogs(saved);
          useAgentStore.getState().restoreAgentSession(saved);
          void useAgentStore.getState().restoreDiffs(saved);
          void useAgentStore.getState().reconcileBackendRun();
          void useEditorStore.getState().restoreEditorSession(saved);
        } else {
          console.log("[App] No saved workspace found, starting empty");
        }
      })
      .catch((err) => {
        console.warn("[App] Failed to load workspace:", err);
      });
  }, []);
}
