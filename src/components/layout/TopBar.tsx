import { useCallback, useState, useEffect } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useThemeStore } from "../../stores/useThemeStore";
import { useTaskStore, type ProjectTaskDefinition } from "../../stores/useTaskStore";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import StatusDot from "../shared/StatusDot";
import ModeSwitch from "../shared/ModeSwitch";
import { isTauriRuntime } from "../../utils/tauri";
import { useProjectTasks } from "../../hooks/useProjectTasks";
import { useRunProjectTask } from "../../hooks/useRunProjectTask";

export default function TopBar() {
  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const llmConfigured = useAgentStore((s) => s.llmConfigured);
  const changeMode = useAgentStore((s) => s.changeMode);
  const stopAgent = useAgentStore((s) => s.stopAgent);
  const focusMode = useLayoutStore((s) => s.focusMode);
  const toggleFocusMode = useLayoutStore((s) => s.toggleFocusMode);
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const theme = useThemeStore((s) => s.theme);
  const toggleTheme = useThemeStore((s) => s.toggleTheme);
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const setWorkspacePath = useLayoutStore((s) => s.setWorkspacePath);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const { tasks } = useProjectTasks();
  const runProjectTask = useRunProjectTask();

  const [isMaximized, setIsMaximized] = useState(false);

  const isRunning =
    agentState !== "idle" && agentState !== "done" && agentState !== "error";

  // 监听窗口最大化状态
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const win = getCurrentWindow();
    win.isMaximized().then(setIsMaximized);
    const unlisten = win.onResized(() => {
      win.isMaximized().then(setIsMaximized);
    });
    return () => { unlisten.then((fn: () => void) => fn()); };
  }, []);

  const handleModeChange = useCallback(
    (mode: "suggest" | "edit" | "auto") => {
      changeMode(mode);
    },
    [changeMode]
  );

  const handleStop = useCallback(() => {
    stopAgent();
  }, [stopAgent]);

  const handleHelp = useCallback(() => {
    window.dispatchEvent(new CustomEvent("toggle-shortcuts-help"));
  }, []);

  // 窗口控制
  const handleMinimize = () => {
    if (isTauriRuntime()) getCurrentWindow().minimize();
  };
  const handleMaximize = () => {
    if (isTauriRuntime()) getCurrentWindow().toggleMaximize();
  };
  const handleClose = () => {
    if (isTauriRuntime()) getCurrentWindow().close();
  };

  // 打开工作目录
  const handleOpenFolder = useCallback(async () => {
    try {
      if (!isTauriRuntime()) return;
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open Workspace Folder",
      });
      if (selected && typeof selected === "string") {
        await invoke("save_workspace_path", { path: selected });
        setWorkspacePath(selected);
        useEditorStore.getState().setWorkspacePath(selected);
      }
    } catch (e) {
      console.warn("Open folder failed:", e);
    }
  }, [setWorkspacePath]);

  // 提取项目名
  const projectName = workspacePath
    ? workspacePath.split(/[/\\]/).pop() || workspacePath
    : "No folder opened";
  const runTask = pickTask(tasks, ["dev", "start", "run"]);
  const debugTask = pickTask(tasks, ["debug", "preview"]);
  const buildTask = pickTask(tasks, ["build"]);
  const testTask = pickTask(tasks, ["test"]);
  const buildStatus = buildTask ? taskRuns[buildTask.id]?.status : undefined;
  const testStatus = testTask ? taskRuns[testTask.id]?.status : undefined;

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-10 px-2 border-b border-surface-border bg-surface-panel no-select flex-shrink-0"
    >
      {/* 左侧：Logo + 项目名 + 打开文件夹 */}
      <div className="flex items-center gap-2 min-w-0">
        <span className="text-accent-purple font-bold text-sm tracking-wide flex-shrink-0">
          ⬨ Agent IDE
        </span>
        <button
          onClick={handleOpenFolder}
          className="text-surface-muted hover:text-surface-text p-1 rounded hover:bg-surface-border/30 text-xs flex-shrink-0"
          title="Open Folder (Ctrl+O)"
        >
          📂
        </button>
        <span
          className="text-xs text-surface-muted truncate max-w-[200px]"
          title={workspacePath}
        >
          {projectName}
        </span>
      </div>

      {/* 中间：项目命令 + Agent 模式切换 */}
      <div className="flex items-center gap-2">
        <div className="flex items-center gap-1 rounded border border-surface-border bg-surface-base px-1 py-0.5">
          <button
            onClick={() => runProjectTask(runTask)}
            disabled={!runTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-text hover:bg-surface-border/40 disabled:cursor-not-allowed disabled:opacity-40"
            title={runTask ? runTask.command : "No run task discovered"}
          >
            Run
          </button>
          <button
            onClick={() => runProjectTask(debugTask)}
            disabled={!debugTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-text hover:bg-surface-border/40 disabled:cursor-not-allowed disabled:opacity-40"
            title={debugTask ? debugTask.command : "No debug task discovered"}
          >
            Debug
          </button>
          <button
            onClick={() => runProjectTask(buildTask)}
            disabled={!buildTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-muted hover:bg-surface-border/40 hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
            title={buildTask ? buildTask.command : "No build task discovered"}
          >
            {buildStatus === "running" ? "Building..." : "Build"}
          </button>
          <button
            onClick={() => runProjectTask(testTask)}
            disabled={!testTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-muted hover:bg-surface-border/40 hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
            title={testTask ? testTask.command : "No test task discovered"}
          >
            {testStatus === "running" ? "Testing..." : "Test"}
          </button>
        </div>
        <ModeSwitch mode={agentMode} onChange={handleModeChange} />
      </div>

      {/* 右侧：状态 + 控制按钮 + 窗口控件 */}
      <div className="flex items-center gap-2">
        {/* LLM 连接状态 */}
        <span
          className={`inline-block w-2 h-2 rounded-full flex-shrink-0 ${llmConfigured ? "bg-green-500 animate-pulse-dot" : "bg-red-500"}`}
          title={llmConfigured ? "LLM Connected" : "LLM Not Configured — open Settings panel to set API credentials"}
        />

        <StatusDot state={agentState} />

        {isRunning && (
          <button
            onClick={handleStop}
            className="px-2.5 py-1 text-xs bg-red-600/70 hover:bg-red-600 text-white rounded transition-colors"
            title="Stop Agent"
          >
            ■ Stop
          </button>
        )}

        <div className="w-px h-4 bg-surface-border" />

        {/* 面板切换按钮组 */}
        <button
          onClick={toggleLeftPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title="Toggle Explorer (Ctrl+Shift+E)"
        >
          📁
        </button>
        <button
          onClick={toggleRightPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title="Toggle Agent Panel (Ctrl+Shift+X)"
        >
          🤖
        </button>
        <button
          onClick={toggleBottomPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title="Toggle Terminal (Ctrl+`)"
        >
          ⬜
        </button>
        <button
          onClick={toggleFocusMode}
          className={`text-xs transition-colors p-0.5 ${
            focusMode ? "text-accent-purple" : "text-surface-muted hover:text-surface-text"
          }`}
          title="Focus Mode (Ctrl+Shift+F)"
        >
          ⊡
        </button>

        <div className="w-px h-4 bg-surface-border" />

        {/* 主题切换 */}
        <button
          onClick={toggleTheme}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={`Switch to ${theme === "dark" ? "Light" : "Dark"} Theme`}
        >
          {theme === "dark" ? "☀" : "🌙"}
        </button>

        {/* 快捷键帮助 */}
        <button
          onClick={handleHelp}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title="Keyboard Shortcuts (F1)"
        >
          ?
        </button>

        {/* 窗口控制按钮 */}
        <div className="flex items-center ml-1" data-tauri-drag-region="false">
          <button
            onClick={handleMinimize}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-surface-text hover:bg-surface-border/30 transition-colors text-sm"
            title="Minimize"
          >
            ─
          </button>
          <button
            onClick={handleMaximize}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-surface-text hover:bg-surface-border/30 transition-colors text-sm"
            title={isMaximized ? "Restore" : "Maximize"}
          >
            {isMaximized ? "❐" : "□"}
          </button>
          <button
            onClick={handleClose}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-white hover:bg-red-600 transition-colors text-sm"
            title="Close"
          >
            ✕
          </button>
        </div>
      </div>
    </div>
  );
}

function pickTask(tasks: ProjectTaskDefinition[], names: string[]) {
  const normalized = names.map((name) => name.toLowerCase());
  return (
    tasks.find((task) => normalized.includes(task.label.toLowerCase())) ??
    tasks.find((task) => normalized.some((name) => task.id.toLowerCase().includes(name)))
  );
}
