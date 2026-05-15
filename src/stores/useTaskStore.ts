import { create } from "zustand";

export type ProjectTaskKind = "build" | "test" | "lint" | "run" | "debug";

export interface ProjectTaskDefinition {
  id: ProjectTaskKind;
  label: string;
  command: string;
  description: string;
}

export interface QueuedTerminalCommand {
  id: string;
  terminalId: string;
  command: string;
  taskId: ProjectTaskKind;
  createdAt: number;
}

interface TaskStore {
  lastTask: QueuedTerminalCommand | null;
  pendingTerminalCommands: QueuedTerminalCommand[];
  queueTerminalCommand: (taskId: ProjectTaskKind, command: string, terminalId?: string) => QueuedTerminalCommand;
  consumeTerminalCommands: (terminalId: string) => QueuedTerminalCommand[];
}

export const PROJECT_TASKS: ProjectTaskDefinition[] = [
  {
    id: "build",
    label: "Build",
    command: "npm run build",
    description: "Compile TypeScript and create the production web build.",
  },
  {
    id: "test",
    label: "Test",
    command: "cd src-tauri; cargo test; cd ..",
    description: "Run the Rust backend test suite.",
  },
  {
    id: "lint",
    label: "Lint",
    command: "npx tsc --noEmit",
    description: "Run TypeScript checking without emitting files.",
  },
  {
    id: "run",
    label: "Run",
    command: "npm run tauri -- dev",
    description: "Start the real Tauri IDE runtime.",
  },
  {
    id: "debug",
    label: "Debug",
    command: "npm run dev",
    description: "Start the Vite web preview for frontend debugging.",
  },
];

export const useTaskStore = create<TaskStore>((set, get) => ({
  lastTask: null,
  pendingTerminalCommands: [],

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
