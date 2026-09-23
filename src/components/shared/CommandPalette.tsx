import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLogStore } from "../../stores/useLogStore";
import type { AgentViewId } from "../../stores/useLayoutStore";
import { useThemeStore } from "../../stores/useThemeStore";
import type { AgentMode } from "../../types/agent";
import type { ProjectTaskDefinition } from "../../stores/useTaskStore";
import { isTauriRuntime } from "../../utils/tauri";
import { useT } from "../../i18n";
import { describeTabs, selectionUrlOrError } from "../../utils/browserTabs";
import type { BrowserTab } from "../../types/browser";

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
  const t = useT();
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
          placeholder={t("palette.search")}
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
              {t("palette.noMatch")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function usePaletteCommands(runProjectTask: (task: ProjectTaskDefinition | undefined) => void | Promise<void>, tasks: ProjectTaskDefinition[]) {
  const t = useT();
  const leftVisible = useLayoutStore((s) => s.leftVisible);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const setLeftTab = useLayoutStore((s) => s.setLeftTab);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const setAgentView = useLayoutStore((s) => s.setAgentView);
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const toggleFocusMode = useLayoutStore((s) => s.toggleFocusMode);
  const performanceOverlay = useLayoutStore((s) => s.performanceOverlay);
  const togglePerformanceOverlay = useLayoutStore((s) => s.togglePerformanceOverlay);
  const setWorkspacePath = useLayoutStore((s) => s.setWorkspacePath);
  const toggleTheme = useThemeStore((s) => s.toggleTheme);
  const stopAgent = useAgentStore((s) => s.stopAgent);
  const changeMode = useAgentStore((s) => s.changeMode);
  const agentState = useAgentStore((s) => s.state);
  const pendingUndo = useAgentStore((s) => s.pendingUndo);
  const undoLastApply = useAgentStore((s) => s.undoLastApply);
  const startNewSession = useAgentStore((s) => s.startNewSession);
  const selectedText = useEditorStore((s) => s.selectedText);
  const addLog = useLogStore((s) => s.addLog);

  /**
   * 浏览器动作的结果走日志面板，并把面板切过去。
   *
   * 不用 alert：这两条命令的失败信息是可执行的（"Chrome 要带
   * --remote-debugging-port 启动"），需要能留在屏幕上被读完、被复制。
   */
  const reportBrowser = useMemo(
    () =>
      (level: "info" | "error", message: string, details?: string) => {
        addLog({
          time: new Date().toLocaleTimeString(),
          level,
          source: "system",
          message,
          details,
        });
        setBottomTab("logs");
      },
    [addLog, setBottomTab]
  );

  return useMemo<PaletteCommand[]>(() => {
    const commands: PaletteCommand[] = [
      {
        id: "browser.open-selection",
        title: t("palette.browser.open"),
        subtitle: t("palette.browser.open.sub"),
        group: t("palette.group.browser"),
        keywords: ["chrome", "url", "open", "cdp", "preview"],
        run: async () => {
          if (!isTauriRuntime()) return;
          const { url, error } = selectionUrlOrError(selectedText);
          if (error) {
            reportBrowser("error", t(error.key, error.params));
            return;
          }
          // 走到这里 `url` 一定有值，这一句只是给类型收窄用
          if (!url) return;
          try {
            const tab = await invoke<BrowserTab>("browser_open_url", { url });
            reportBrowser("info", t("palette.browser.opened", { url: tab.url }));
          } catch (e) {
            reportBrowser("error", t("palette.browser.openFailed"), String(e));
          }
        },
      },
      {
        id: "browser.list-tabs",
        title: t("palette.browser.tabs"),
        subtitle: t("palette.browser.tabs.sub"),
        group: t("palette.group.browser"),
        keywords: ["chrome", "tabs", "cdp", "attach"],
        run: async () => {
          if (!isTauriRuntime()) return;
          try {
            const tabs = await invoke<BrowserTab[]>("browser_list_tabs");
            const summary = describeTabs(tabs);
            reportBrowser(
              "info",
              t(summary.key, summary.params),
              tabs.map((tab) => `${tab.title} — ${tab.url}`).join("\n")
            );
          } catch (e) {
            reportBrowser("error", t("palette.browser.unreachable"), String(e));
          }
        },
      },
      {
        id: "workspace.open-folder",
        title: t("palette.workspace.open"),
        subtitle: t("palette.workspace.open.sub"),
        group: t("palette.group.workspace"),
        keywords: ["folder", "project"],
        run: async () => {
          if (!isTauriRuntime()) return;
          const selected = await open({
            directory: true,
            multiple: false,
            title: t("palette.workspace.open"),
          });
          if (selected && typeof selected === "string") {
            await invoke("save_workspace_path", { path: selected });
            setWorkspacePath(selected);
            useEditorStore.getState().setWorkspacePath(selected);
          }
        },
      },
      panelCommand("panel.explorer", t("palette.explorer"), t("palette.group.navigation"), () => {
        setLeftTab("explorer");
        if (!leftVisible) toggleLeftPanel();
      }),
      panelCommand("panel.git", t("palette.git"), t("palette.group.navigation"), () => {
        setLeftTab("git");
        if (!leftVisible) toggleLeftPanel();
      }),
      panelCommand("panel.agent", t("palette.agent"), t("palette.group.navigation"), () => {
        if (!rightVisible) toggleRightPanel();
      }),
      agentViewCommand(
        "panel.agent.task",
        t("palette.agent.task"),
        t("palette.group.agent"),
        "task",
        setAgentView,
        rightVisible,
        toggleRightPanel
      ),
      agentViewCommand(
        "panel.agent.plan",
        t("palette.agent.plan"),
        t("palette.group.agent"),
        "plan",
        setAgentView,
        rightVisible,
        toggleRightPanel
      ),
      agentViewCommand(
        "panel.agent.changes",
        t("palette.agent.changes"),
        t("palette.group.agent"),
        "changes",
        setAgentView,
        rightVisible,
        toggleRightPanel
      ),
      // 历史任务和新建任务同理：面板上是两个 8px 的图标，命令面板是它们唯一带文字的入口。
      // "新建任务"是个动作而不是视图，所以不走 `agentViewCommand`。
      agentViewCommand(
        "panel.agent.sessions",
        t("palette.agent.sessions"),
        t("palette.group.agent"),
        "sessions",
        setAgentView,
        rightVisible,
        toggleRightPanel,
        ["history", "sessions", "tasks", "resume", "previous conversation", "context"]
      ),
      {
        id: "agent.new-session",
        title: t("palette.agent.new"),
        subtitle: t("palette.agent.new.sub"),
        group: t("palette.group.agent"),
        keywords: ["new", "task", "clear", "reset", "conversation", "context", "session"],
        run: () => {
          // 被拒绝时错误已经进 store.error，Agent 面板上的错误条会显示
          void startNewSession().catch(() => undefined);
        },
      },
      // Pipeline 和 Settings 此前只有 Agent 面板上两个 8px 宽的纯图标按钮可以进，
      // 命令面板也不收录它们 —— 于是 provider 配置、权限、花费上限和 MCP 全都只能
      // 靠碰对那个图标才能找到。MCP 更深一层：它在 Settings 表单的最底部，
      // 所以这里把它当作关键词挂到 Settings 上，搜 "mcp" 能直接到。
      agentViewCommand(
        "panel.agent.pipeline",
        t("palette.agent.pipeline"),
        t("palette.group.agent"),
        "pipeline",
        setAgentView,
        rightVisible,
        toggleRightPanel,
        ["stages", "roles", "architect", "coder", "reviewer", "designer", "tester"]
      ),
      agentViewCommand(
        "panel.agent.settings",
        t("palette.agent.settings"),
        t("palette.group.agent"),
        "settings",
        setAgentView,
        rightVisible,
        toggleRightPanel,
        [
          "provider",
          "profile",
          "api key",
          "model",
          "permissions",
          "token cap",
          "spend cap",
          "mcp",
          "context",
        ]
      ),
      panelCommand("panel.terminal", t("palette.terminal"), t("palette.group.navigation"), () => {
        setBottomTab("terminal");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.commands", t("palette.commands"), t("palette.group.navigation"), () => {
        setBottomTab("commands");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.problems", t("palette.problems"), t("palette.group.navigation"), () => {
        setBottomTab("problems");
        if (!bottomVisible) toggleBottomPanel();
      }),
      panelCommand("panel.logs", t("palette.logs"), t("palette.group.navigation"), () => {
        setBottomTab("logs");
        if (!bottomVisible) toggleBottomPanel();
      }),
      {
        id: "layout.focus",
        title: t("palette.focus"),
        group: t("palette.group.view"),
        run: toggleFocusMode,
      },
      {
        id: "theme.toggle",
        title: t("palette.theme"),
        group: t("palette.group.view"),
        run: toggleTheme,
      },
      {
        id: "view.performance-overlay",
        title: performanceOverlay ? t("palette.perf.hide") : t("palette.perf.show"),
        subtitle: t("palette.perf.sub"),
        group: t("palette.group.view"),
        run: togglePerformanceOverlay,
      },
      agentModeCommand(
        "agent.mode.suggest",
        t("palette.mode.suggest"),
        t("palette.group.agent"),
        "suggest",
        changeMode
      ),
      agentModeCommand(
        "agent.mode.auto",
        t("palette.mode.auto"),
        t("palette.group.agent"),
        "auto",
        changeMode
      ),
      {
        id: "agent.undo-apply",
        // 唯一的 Undo 按钮在 Changes 视图里，右面板一收起就没有退路了。
        // 撤销是"刚发现改错了"时要用的东西，不能只有一个入口。
        title: pendingUndo
          ? t("palette.undo.named", { label: pendingUndo.label })
          : t("palette.undo"),
        subtitle: pendingUndo
          ? t("palette.undo.sub", { count: pendingUndo.files.length })
          : t("palette.undo.none"),
        group: t("palette.group.agent"),
        keywords: ["revert", "restore", "rollback"],
        disabled: !pendingUndo,
        run: () => void undoLastApply(),
      },
      {
        id: "agent.stop",
        title: t("palette.stop"),
        subtitle: t("palette.stop.sub"),
        group: t("palette.group.agent"),
        disabled: agentState === "idle" || agentState === "done",
        run: () => void stopAgent(),
      },
    ];

    for (const task of tasks) {
      commands.push({
        id: `task.${task.id}`,
        title: task.label,
        subtitle: task.command,
        group: t("palette.group.task"),
        keywords: [task.id, task.command, task.source],
        run: () => runProjectTask(task),
      });
    }

    return commands;
  }, [
    addLog,
    agentState,
    bottomVisible,
    changeMode,
    leftVisible,
    pendingUndo,
    performanceOverlay,
    rightVisible,
    runProjectTask,
    selectedText,
    setBottomTab,
    setAgentView,
    setLeftTab,
    setWorkspacePath,
    stopAgent,
    t,
    tasks,
    toggleBottomPanel,
    toggleFocusMode,
    toggleLeftPanel,
    togglePerformanceOverlay,
    toggleRightPanel,
    toggleTheme,
    startNewSession,
    undoLastApply,
  ]);
}

function panelCommand(id: string, title: string, group: string, run: () => void): PaletteCommand {
  return { id, title, group, run };
}

function agentViewCommand(
  id: string,
  title: string,
  group: string,
  view: AgentViewId,
  setAgentView: (view: AgentViewId) => void,
  rightVisible: boolean,
  toggleRightPanel: () => void,
  keywords?: string[]
): PaletteCommand {
  return {
    id,
    title,
    group,
    keywords,
    run: () => {
      setAgentView(view);
      if (!rightVisible) toggleRightPanel();
    },
  };
}

function agentModeCommand(
  id: string,
  title: string,
  group: string,
  mode: AgentMode,
  changeMode: (mode: AgentMode) => Promise<void>
): PaletteCommand {
  return {
    id,
    title,
    group,
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
    // id 也参与匹配：标题跟着界面语言走，而 `panel.explorer`、`theme.toggle` 这些
    // 不会。少了它，界面切成中文之后搜 "explorer" 就一条都找不到。
    command.id,
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
