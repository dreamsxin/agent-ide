import { useLogStore } from "../stores/useLogStore";
import { useProblemStore, type ProblemEntry } from "../stores/useProblemStore";
import { useTaskStore, type ProjectTaskRunState } from "../stores/useTaskStore";
import type { LogEntry } from "../types/project";

export interface IdeRuntimeContextOptions {
  includeFailedTask?: boolean;
  includeProblems?: boolean;
  includeTerminalOutput?: boolean;
  includeLogs?: boolean;
}

const DEFAULT_RUNTIME_CONTEXT_OPTIONS: Required<IdeRuntimeContextOptions> = {
  includeFailedTask: true,
  includeProblems: true,
  includeTerminalOutput: true,
  includeLogs: true,
};

export function withIdeRuntimeContext(prompt: string, options?: IdeRuntimeContextOptions) {
  const ideRuntimeContext = buildIdeRuntimeContext(options);
  return ideRuntimeContext
    ? `${prompt}\n\n=== IDE Runtime Context ===\n${ideRuntimeContext}`
    : prompt;
}

export function buildProblemFixPrompt(problem?: ProblemEntry) {
  const target = problem
    ? [
        `Target problem:`,
        `- file: ${problem.file}`,
        `- location: ${problem.line}:${problem.column}`,
        `- severity: ${problem.severity}`,
        `- source: ${problem.source}`,
        `- message: ${problem.message}`,
      ].join("\n")
    : "Target: all current Problems entries.";

  return [
    "Fix the current IDE problem(s). Use the Problems list, recent terminal output, and failed command context below. Return proposed code changes as reviewable diffs when changes are needed.",
    "",
    target,
  ].join("\n");
}

export function buildTaskFailureFixPrompt(task: ProjectTaskRunState) {
  return [
    "Fix the failing project command. Use the command output, Problems list, recent terminal output, and logs below. Return proposed code changes as reviewable diffs when changes are needed.",
    "",
    `Command: ${task.command}`,
    `Exit code: ${task.exitCode ?? "unknown"}`,
  ].join("\n");
}

export function buildIdeRuntimeContext(options?: IdeRuntimeContextOptions) {
  const resolvedOptions = { ...DEFAULT_RUNTIME_CONTEXT_OPTIONS, ...options };
  const taskState = useTaskStore.getState();
  const problems = useProblemStore.getState().problems;
  const logs = useLogStore.getState().logs;

  const latestFailedTask = Object.values(taskState.taskRuns)
    .filter((task) => task.status === "failed")
    .sort((a, b) => (b.finishedAt ?? b.startedAt) - (a.finishedAt ?? a.startedAt))[0];

  const sections: string[] = [];
  if (resolvedOptions.includeFailedTask && latestFailedTask) {
    sections.push(formatFailedTask(latestFailedTask));
  }

  if (resolvedOptions.includeProblems && problems.length > 0) {
    sections.push(
      [
        "Current Problems:",
        ...problems.slice(0, 20).map((problem) => {
          return `- [${problem.severity}] ${problem.file}:${problem.line}:${problem.column} ${problem.message}`;
        }),
      ].join("\n")
    );
  }

  const terminalOutput = resolvedOptions.includeTerminalOutput
    ? Object.entries(taskState.terminalOutput)
    .sort(([a], [b]) => (a === "main" ? -1 : b === "main" ? 1 : a.localeCompare(b)))
    .map(([id, output]) => {
      const trimmed = output.trim();
      return trimmed ? `Terminal ${id}:\n${tail(trimmed, 2000)}` : "";
    })
    .filter(Boolean)
    .join("\n\n")
    : "";
  if (terminalOutput) {
    sections.push(`Recent Terminal Output:\n${tail(terminalOutput, 4000)}`);
  }

  const relevantLogs = logs
    .filter((log) => log.level === "error" || log.level === "warn")
    .slice(-8);
  if (resolvedOptions.includeLogs && relevantLogs.length > 0) {
    sections.push(
      [
        "Recent Error/Warning Logs:",
        ...relevantLogs.map(formatLogLine),
      ].join("\n")
    );
  }

  if (sections.length === 0) return "";
  return [
    "The following IDE runtime context is current and should be used when diagnosing build/test/runtime failures.",
    ...sections,
  ].join("\n\n");
}

function formatFailedTask(task: ProjectTaskRunState) {
  return [
    "Latest Failed Project Command:",
    `- command: ${task.command}`,
    `- exitCode: ${task.exitCode ?? "unknown"}`,
    `- durationMs: ${task.durationMs ?? "unknown"}`,
    "Output:",
    tail(task.output ?? "", 4000),
  ].join("\n");
}

function formatLogLine(log: LogEntry) {
  const details = log.details ? ` ${tail(log.details, 400)}` : "";
  return `- [${log.level}] ${log.time} ${log.message}${details}`;
}

function tail(value: string, maxLength: number) {
  if (value.length <= maxLength) return value;
  return value.slice(value.length - maxLength);
}
