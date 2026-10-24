import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLayoutStore } from "../stores/useLayoutStore";
import { useLogStore } from "../stores/useLogStore";
import { useProblemStore } from "../stores/useProblemStore";
import { useTaskStore, type ProjectTaskDefinition } from "../stores/useTaskStore";
import { parseTerminalProblems } from "../utils/terminalProblemParser";
import { isTauriRuntime } from "../utils/tauri";
import { useT } from "../i18n";

interface RunProjectTaskResult {
  command: string;
  exitCode: number | null;
  durationMs: number;
  stdout: string;
  stderr: string;
}

export function useRunProjectTask() {
  const t = useT();
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const queueTerminalSession = useTaskStore((s) => s.queueTerminalSession);
  const startTaskRun = useTaskStore((s) => s.startTaskRun);
  const finishTaskRun = useTaskStore((s) => s.finishTaskRun);
  const addLog = useLogStore((s) => s.addLog);
  const replaceProblems = useProblemStore((s) => s.replaceProblems);
  const clearProblems = useProblemStore((s) => s.clearProblems);

  return useCallback(
    async (task: ProjectTaskDefinition | undefined) => {
      if (!task || !isTauriRuntime()) return;
      clearProblems("test");

      if (shouldUseCommandRunner(task)) {
        startTaskRun(task.id, task.command, task.label);
        addLog({
          time: new Date().toLocaleTimeString(),
          level: "info",
          source: "system",
          message: t("tasks.log.running", { label: task.label }),
          details: task.command,
        });

        try {
          const result = await invoke<RunProjectTaskResult>("run_project_task", {
            request: { command: task.command, cwd: workspacePath || null },
          });
          const output = [result.stdout, result.stderr].filter(Boolean).join("\n");
          const status = result.exitCode === 0 ? "success" : "failed";
          finishTaskRun(task.id, status, {
            exitCode: result.exitCode,
            durationMs: result.durationMs,
            output,
          });
          replaceProblems("test", parseTerminalProblems(output, "task"));
          addLog({
            time: new Date().toLocaleTimeString(),
            level: status === "success" ? "success" : "error",
            source: "system",
            message: t("tasks.log.finished", {
              label: task.label,
              status: t(`tasks.status.${status}`),
              ms: result.durationMs,
            }),
            details: output.slice(0, 4000),
          });
        } catch (err) {
          finishTaskRun(task.id, "failed", {
            exitCode: null,
            durationMs: 0,
            output: String(err),
          });
          addLog({
            time: new Date().toLocaleTimeString(),
            level: "error",
            source: "system",
            message: t("tasks.log.failed", { label: task.label }),
            details: String(err),
          });
        }
        return;
      }

      if (!bottomVisible) {
        toggleBottomPanel();
      }
      setBottomTab("terminal");
      queueTerminalSession(task.id, task.label, task.command, workspacePath || "");
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "info",
        source: "system",
        message: t("tasks.log.terminal", { label: task.label }),
        details: task.command,
      });
    },
    [
      addLog,
      bottomVisible,
      clearProblems,
      finishTaskRun,
      queueTerminalSession,
      replaceProblems,
      setBottomTab,
      startTaskRun,
      t,
      toggleBottomPanel,
      workspacePath,
    ]
  );
}

function shouldUseCommandRunner(task: ProjectTaskDefinition) {
  const value = `${task.id} ${task.label}`.toLowerCase();
  return ["build", "test", "lint", "check", "typecheck"].some((name) => value.includes(name));
}
