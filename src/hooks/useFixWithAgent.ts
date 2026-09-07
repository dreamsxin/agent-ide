import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "../utils/tauri";
import { useAgentStore } from "../stores/useAgentStore";
import { useEditorStore } from "../stores/useEditorStore";
import { useLayoutStore } from "../stores/useLayoutStore";
import type { ProblemEntry } from "../stores/useProblemStore";
import type { ProjectTaskRunState } from "../stores/useTaskStore";
import {
  buildProblemExplainPrompt,
  buildProblemFixPrompt,
  buildTaskFailureFixPrompt,
  withIdeRuntimeContext,
} from "../utils/agentRuntimeContext";

export function useFixWithAgent() {
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const addMessage = useAgentStore((s) => s.addMessage);
  const agentState = useAgentStore((s) => s.state);
  const activeFile = useEditorStore((s) => s.activeFile);
  const fileContents = useEditorStore((s) => s.fileContents);
  const selectedText = useEditorStore((s) => s.selectedText);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const setAgentView = useLayoutStore((s) => s.setAgentView);

  const isAgentBusy =
    agentState !== "idle" &&
    agentState !== "done" &&
    agentState !== "error" &&
    agentState !== "waiting_user";

  const sendFixPrompt = useCallback(
    async (prompt: string) => {
      if (isAgentBusy) return;
      setAgentView("task");
      if (!rightVisible) {
        toggleRightPanel();
      }

      const fullPrompt = withIdeRuntimeContext(prompt);
      addMessage({
        id: `fix-${Date.now()}`,
        role: "user",
        content: fullPrompt,
        timestamp: Date.now(),
      });

      await sendPrompt({
        prompt: fullPrompt,
        contextFiles: activeFile ? [activeFile] : [],
        activeFile: activeFile ?? undefined,
        activeFileContent: activeFile ? fileContents[activeFile] : undefined,
        selection: selectedText ?? undefined,
        ideMode: "code",
      });
    },
    [
      activeFile,
      addMessage,
      fileContents,
      isAgentBusy,
      rightVisible,
      setAgentView,
      selectedText,
      sendPrompt,
      toggleRightPanel,
    ]
  );

  const explainProblem = useCallback(
    async (problem?: ProblemEntry) => {
      await sendFixPrompt(buildProblemExplainPrompt(problem));
    },
    [sendFixPrompt]
  );

  const fixProblem = useCallback(
    async (problem?: ProblemEntry) => {
      await sendFixPrompt(buildProblemFixPrompt(problem));
    },
    [sendFixPrompt]
  );

  const fixTaskFailure = useCallback(
    async (task: ProjectTaskRunState) => {
      // 提示词以后端那份为准。同一段提示曾在 Rust 和 TypeScript 各存一份，
      // 两边对"输出太长时保哪一半"给出过相反答案（CLI 用的那半是错的）。
      // TS 版本现在只在非 Tauri 环境或 IPC 失败时兜底。
      let prompt: string | null = null;
      if (isTauriRuntime()) {
        try {
          prompt = await invoke<string>("agent_repair_prompt", {
            request: {
              command: task.command,
              exitCode: task.exitCode ?? null,
              output: task.output ?? "",
            },
          });
        } catch {
          prompt = null;
        }
      }
      await sendFixPrompt(prompt ?? buildTaskFailureFixPrompt(task));
    },
    [sendFixPrompt]
  );

  return {
    isAgentBusy,
    sendFixPrompt,
    explainProblem,
    fixProblem,
    fixTaskFailure,
  };
}
