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

interface TaskStore {
  lastTask: QueuedTerminalCommand | null;
  pendingTerminalCommands: QueuedTerminalCommand[];
  discoveredTasks: ProjectTaskDefinition[];
  taskDiscoveryLoading: boolean;
  taskDiscoveryLoaded: boolean;
  taskDiscoveryError: string | null;
  setDiscoveredTasks: (tasks: ProjectTaskDefinition[]) => void;
  setTaskDiscoveryState: (loading: boolean, error?: string | null, loaded?: boolean) => void;
  queueTerminalCommand: (taskId: string, command: string, terminalId?: string) => QueuedTerminalCommand;
  consumeTerminalCommands: (terminalId: string) => QueuedTerminalCommand[];
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
  discoveredTasks: [],
  taskDiscoveryLoading: false,
  taskDiscoveryLoaded: false,
  taskDiscoveryError: null,

  setDiscoveredTasks: (discoveredTasks) => set({ discoveredTasks }),
  setTaskDiscoveryState: (taskDiscoveryLoading, taskDiscoveryError = null, taskDiscoveryLoaded) =>
    set((state) => ({
      taskDiscoveryLoading,
      taskDiscoveryError,
      taskDiscoveryLoaded: taskDiscoveryLoaded ?? state.taskDiscoveryLoaded,
    })),

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
}));
