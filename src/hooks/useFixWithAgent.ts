import { useCallback } from "react";
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

  const isAgentBusy =
    agentState !== "idle" &&
    agentState !== "done" &&
    agentState !== "error" &&
    agentState !== "waiting_user";

  const sendFixPrompt = useCallback(
    async (prompt: string) => {
      if (isAgentBusy) return;
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
      await sendFixPrompt(buildTaskFailureFixPrompt(task));
    },
    [sendFixPrompt]
  );

  return {
    isAgentBusy,
    explainProblem,
    fixProblem,
    fixTaskFailure,
  };
}
