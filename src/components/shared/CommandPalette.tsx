import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useThemeStore } from "../../stores/useThemeStore";
import type { AgentMode } from "../../types/agent";
import type { ProjectTaskDefinition } from "../../stores/useTaskStore";
import { isTauriRuntime } from "../../utils/tauri";

export interface PaletteCommand {
  id: string;
  title: string;
  subtitle?: string;
  group: string;
  keywords?: string[];
  disabled?: boolean;
  run: () => void | Promise<void>;
}

interface CommandPaletteProps {
  visible: boolean;
  commands: PaletteCommand[];
  onClose: () => void;
}

export default function CommandPalette({ visible, commands, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!visible) return;
    setQuery("");
    setSelectedIndex(0);
    window.setTimeout(() => inputRef.current?.focus(), 0);
  }, [visible]);

  const filtered = useMemo(() => {
    const value = query.trim().toLowerCase();
    const candidates = commands.filter((command) => !command.disabled);
    if (!value) return candidates;
    return candidates
      .map((command) => ({ command, score: scoreCommand(command, value) }))
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score || a.command.title.localeCompare(b.command.title))
      .map((item) => item.command);
  }, [commands, query]);

  useEffect(() => {
    setSelectedIndex((index) => Math.min(index, Math.max(filtered.length - 1, 0)));
  }, [filtered.length]);

  if (!visible) return null;

  const runSelected = async () => {
    const command = filtered[selectedIndex];
    if (!command) return;
    await command.run();
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-[100] bg-black/35"
      onMouseDown={onClose}
    >
      <div
        className="mx-auto mt-[10vh] w-[min(720px,calc(100vw-32px))] overflow-hidden rounded border border-surface-border bg-surface-panel shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              onClose();
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelectedIndex((index) => Math.min(index + 1, filtered.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelectedIndex((index) => Math.max(index - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              void runSelected();
            }
          }}
          placeholder="Search commands..."
          className="w-full border-b border-surface-border bg-surface-base px-4 py-3 text-sm text-surface-text outline-none placeholder:text-surface-muted"
        />
        <div className="max-h-[55vh] overflow-auto p-1">
          {filtered.length > 0 ? (
            filtered.map((command, index) => (
              <button
                key={command.id}
                type="button"
                onMouseEnter={() => setSelectedIndex(index)}
                onClick={() => void runSelectedCommand(command, onClose)}
                className={`grid w-full grid-cols-[96px_minmax(0,1fr)] gap-3 rounded px-3 py-2 text-left text-xs ${
                  index === selectedIndex
                    ? "bg-accent-blue/15 text-surface-text"
                    : "text-surface-muted hover:bg-surface-border/30 hover:text-surface-text"
                }`}
              >
                <span className="truncate text-[10px] uppercase tracking-wide text-surface-muted">
                  {command.group}
                </span>
                <span className="min-w-0">
                  <span className="block truncate font-medium">{command.title}</span>
                  {command.subtitle && (
                    <span className="block truncate text-[11px] text-surface-muted">
                      {command.subtitle}
                    </span>
                  )}
                </span>
              </button>
            ))
          ) : (
            <div className="px-4 py-8 text-center text-xs text-surface-muted">
              No matching commands
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function usePaletteCommands(runProjectTask: (task: ProjectTaskDefinition | undefined) => void | Promise<void>, tasks: ProjectTaskDefinition[]) {
  const leftVisible = useLayoutStore((s) => s.leftVisible);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const setLeftTab = useLayoutStore((s) => s.setLeftTab);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const toggleFocusMode = useLayoutStore((s) => s.toggleFocusMode);
  const setWorkspacePath = useLayoutStore((s) => s.setWorkspacePath);
  const toggleTheme = useThemeStore((s) => s.toggleTheme);
  const stopAgent = useAgentStore((s) => s.stopAgent);
  const changeMode = useAgentStore((s) => s.changeMode);
  const agentState = useAgentStore((s) => s.state);

  return useMemo<PaletteCommand[]>(() => {
    const commands: PaletteCommand[] = [
      {
        id: "workspace.open-folder",
        title: "Open Workspace Folder",
        subtitle: "Choose a folder and make it the active workspace",
        group: "Workspace",
        keywords: ["folder", "project"],
        run: async () => {
          if (!isTauriRuntime()) return;
          const selected = await open({ directory: true, multiple: false, title: "Open Workspace Folder" });
          if (selected && typeof selected === "string") {
            await invoke("save_workspace_path", { path: selected });
            setWorkspacePath(selected);
            useEditorStore.getState().setWorkspacePath(selected);
          }
        },
      },
      panelCommand("panel.explorer", "Show Explorer", "Navigation", () => {
        setLeftTab("explorer");
        if (!leftVisible) toggleLeftPanel();
      }),
      panelCommand("panel.git", "Show Source Control", "Navigation", () => {
        setLeftTab("git");
        if (!leftVisible) toggleLeftPanel();
      }),
      panelCommand("panel.agent", "Show Agent", "Navigation", () => {
        if (!rightVisible) toggleRightPanel();
      }),
      panelCommand("panel.terminal", "Show Terminal", "Navigation", () => {
        setBottomTab("terminal");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.commands", "Show Commands", "Navigation", () => {
        setBottomTab("commands");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.problems", "Show Problems", "Navigation", () => {
        setBottomTab("problems");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.logs", "Show Logs", "Navigation", () => {
        setBottomTab("logs");
        if (!bottomVisible) toggleBottomPanel();
      }),
      {
        id: "layout.focus",
        title: "Toggle Focus Mode",
        group: "View",
        run: toggleFocusMode,
      },
      {
        id: "theme.toggle",
        title: "Toggle Theme",
        group: "View",
        run: toggleTheme,
      },
      agentModeCommand("agent.mode.suggest", "Set Agent Mode: Suggest", "suggest", changeMode),
      agentModeCommand("agent.mode.edit", "Set Agent Mode: Edit", "edit", changeMode),
      agentModeCommand("agent.mode.auto", "Set Agent Mode: Auto", "auto", changeMode),
      {
        id: "agent.stop",
        title: "Stop Agent",
        subtitle: "Cancel the current Agent run",
        group: "Agent",
        disabled: agentState === "idle" || agentState === "done",
        run: () => void stopAgent(),
      },
    ];

    for (const task of tasks) {
      commands.push({
        id: `task.${task.id}`,
        title: task.label,
        subtitle: task.command,
        group: "Project Command",
        keywords: [task.id, task.command, task.source],
        run: () => runProjectTask(task),
      });
    }

    return commands;
  }, [
    agentState,
    bottomVisible,
    changeMode,
    leftVisible,
    rightVisible,
    runProjectTask,
    setBottomTab,
    setLeftTab,
    setWorkspacePath,
    stopAgent,
    tasks,
    toggleBottomPanel,
    toggleFocusMode,
    toggleLeftPanel,
    toggleRightPanel,
    toggleTheme,
  ]);
}

function panelCommand(id: string, title: string, group: string, run: () => void): PaletteCommand {
  return { id, title, group, run };
}

function agentModeCommand(
  id: string,
  title: string,
  mode: AgentMode,
  changeMode: (mode: AgentMode) => Promise<void>
): PaletteCommand {
  return {
    id,
    title,
    group: "Agent",
    run: () => void changeMode(mode),
  };
}

async function runSelectedCommand(command: PaletteCommand, onClose: () => void) {
  await command.run();
  onClose();
}

function scoreCommand(command: PaletteCommand, query: string) {
  const haystack = [
    command.title,
    command.subtitle,
    command.group,
    ...(command.keywords ?? []),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  if (haystack === query) return 100;
  if (haystack.startsWith(query)) return 80;
  if (command.title.toLowerCase().includes(query)) return 60;
  if (haystack.includes(query)) return 30;
  return fuzzyIncludes(haystack, query) ? 10 : 0;
}

function fuzzyIncludes(value: string, query: string) {
  let index = 0;
  for (const char of value) {
    if (char === query[index]) index += 1;
    if (index === query.length) return true;
  }
  return false;
}
