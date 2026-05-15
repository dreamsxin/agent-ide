import { create } from "zustand";

export type ProjectTaskKind = "build" | "test" | "lint" | "run" | "debug";

export interface ProjectTaskDefinition {
  id: string;
  label: string;
  command: string;
  description: string;
  source: string;
}

export interface QueuedTerminalCommand {
  id: string;
  terminalId: string;
  command: string;
  taskId: string;
  createdAt: number;
}

export type ProjectTaskStatus = "idle" | "running" | "success" | "failed";

export interface ProjectTaskRunState {
  runId?: string;
  taskId: string;
  label?: string;
  status: ProjectTaskStatus;
  command: string;
  startedAt: number;
  finishedAt?: number;
  exitCode?: number | null;
  durationMs?: number;
  output?: string;
}

export interface ProjectTaskRunHistoryEntry extends ProjectTaskRunState {
  runId: string;
  taskId: string;
  label: string;
}

interface TaskStore {
  lastTask: QueuedTerminalCommand | null;
  pendingTerminalCommands: QueuedTerminalCommand[];
  terminalOutput: Record<string, string>;
  discoveredTasks: ProjectTaskDefinition[];
  taskDiscoveryLoading: boolean;
  taskDiscoveryLoaded: boolean;
  taskDiscoveryError: string | null;
  taskRuns: Record<string, ProjectTaskRunState>;
  taskRunHistory: ProjectTaskRunHistoryEntry[];
  setDiscoveredTasks: (tasks: ProjectTaskDefinition[]) => void;
  setTaskDiscoveryState: (loading: boolean, error?: string | null, loaded?: boolean) => void;
  startTaskRun: (taskId: string, command: string, label?: string) => string;
  finishTaskRun: (
    taskId: string,
    status: Exclude<ProjectTaskStatus, "idle" | "running">,
    updates: Pick<ProjectTaskRunState, "exitCode" | "durationMs" | "output">
  ) => void;
  clearTaskRunHistory: () => void;
  queueTerminalCommand: (taskId: string, command: string, terminalId?: string) => QueuedTerminalCommand;
  consumeTerminalCommands: (terminalId: string) => QueuedTerminalCommand[];
  appendTerminalOutput: (terminalId: string, output: string) => void;
  clearTerminalOutput: (terminalId?: string) => void;
}

export const PROJECT_TASKS: ProjectTaskDefinition[] = [
  {
    id: "build",
    label: "Build",
    command: "npm run build",
    description: "Compile TypeScript and create the production web build.",
    source: "default",
  },
  {
    id: "test",
    label: "Test",
    command: "cd src-tauri; cargo test; cd ..",
    description: "Run the Rust backend test suite.",
    source: "default",
  },
  {
    id: "lint",
    label: "Lint",
    command: "npx tsc --noEmit",
    description: "Run TypeScript checking without emitting files.",
    source: "default",
  },
  {
    id: "run",
    label: "Run",
    command: "npm run tauri -- dev",
    description: "Start the real Tauri IDE runtime.",
    source: "default",
  },
  {
    id: "debug",
    label: "Debug",
    command: "npm run dev",
    description: "Start the Vite web preview for frontend debugging.",
    source: "default",
  },
];

export const useTaskStore = create<TaskStore>((set, get) => ({
  lastTask: null,
  pendingTerminalCommands: [],
  terminalOutput: {},
  discoveredTasks: [],
  taskDiscoveryLoading: false,
  taskDiscoveryLoaded: false,
  taskDiscoveryError: null,
  taskRuns: {},
  taskRunHistory: [],

  setDiscoveredTasks: (discoveredTasks) => set({ discoveredTasks }),
  setTaskDiscoveryState: (taskDiscoveryLoading, taskDiscoveryError = null, taskDiscoveryLoaded) =>
    set((state) => ({
      taskDiscoveryLoading,
      taskDiscoveryError,
      taskDiscoveryLoaded: taskDiscoveryLoaded ?? state.taskDiscoveryLoaded,
    })),

  startTaskRun: (taskId, command, label = taskId) => {
    const runId = `run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    set((state) => ({
      taskRuns: {
        ...state.taskRuns,
        [taskId]: {
          runId,
          taskId,
          label,
          command,
          status: "running",
          startedAt: Date.now(),
        },
      },
    }));
    return runId;
  },

  finishTaskRun: (taskId, status, updates) =>
    set((state) => {
      const finishedAt = Date.now();
      const current = state.taskRuns[taskId] ?? {
        runId: `run-${finishedAt}-${Math.random().toString(36).slice(2, 8)}`,
        taskId,
        label: taskId,
        command: "",
        startedAt: finishedAt,
      };
      const nextRun: ProjectTaskRunState = {
        ...current,
        ...updates,
        status,
        finishedAt,
      };
      const historyEntry: ProjectTaskRunHistoryEntry = {
        runId: nextRun.runId ?? `run-${finishedAt}-${Math.random().toString(36).slice(2, 8)}`,
        taskId,
        label: nextRun.label ?? taskId,
        command: nextRun.command,
        startedAt: nextRun.startedAt,
        finishedAt,
        status,
        exitCode: nextRun.exitCode,
        durationMs: nextRun.durationMs,
        output: nextRun.output,
      };

      return {
        taskRuns: {
          ...state.taskRuns,
          [taskId]: nextRun,
        },
        taskRunHistory: [historyEntry, ...state.taskRunHistory].slice(0, 100),
      };
    }),

  clearTaskRunHistory: () => set({ taskRunHistory: [] }),

  queueTerminalCommand: (taskId, command, terminalId = "main") => {
    const queued: QueuedTerminalCommand = {
      id: `task-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      terminalId,
      command,
      taskId,
      createdAt: Date.now(),
    };
    set((state) => ({
      lastTask: queued,
      pendingTerminalCommands: [...state.pendingTerminalCommands, queued],
    }));
    return queued;
  },

  consumeTerminalCommands: (terminalId) => {
    const commands = get().pendingTerminalCommands.filter(
      (command) => command.terminalId === terminalId
    );
    if (commands.length === 0) return [];
    set((state) => ({
      pendingTerminalCommands: state.pendingTerminalCommands.filter(
        (command) => command.terminalId !== terminalId
      ),
    }));
    return commands;
  },

  appendTerminalOutput: (terminalId, output) =>
    set((state) => {
      const previous = state.terminalOutput[terminalId] ?? "";
      const next = `${previous}${output}`.slice(-12000);
      return {
        terminalOutput: {
          ...state.terminalOutput,
          [terminalId]: next,
        },
      };
    }),

  clearTerminalOutput: (terminalId) =>
    set((state) => {
      if (!terminalId) return { terminalOutput: {} };
      const { [terminalId]: _, ...rest } = state.terminalOutput;
      return { terminalOutput: rest };
    }),
}));
