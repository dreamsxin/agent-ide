import { useCallback, useState, useEffect } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLspStore } from "../../stores/useLspStore";
import { useThemeStore } from "../../stores/useThemeStore";
import { useTaskStore, type ProjectTaskDefinition } from "../../stores/useTaskStore";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import ModeSwitch from "../shared/ModeSwitch";
import type { AgentMode } from "../../types/agent";
import { useLocaleStore, useT } from "../../i18n";
import { isTauriRuntime } from "../../utils/tauri";
import { agentRunIsLive, isAgentBusy } from "../../utils/agentExperience";
import { getLspStatus, probeLsp, type LspStatusSnapshot } from "../../utils/lspClient";
import { useProjectTasks } from "../../hooks/useProjectTasks";
import { useRunProjectTask } from "../../hooks/useRunProjectTask";
import {
  Bot,
  CircleHelp,
  Command,
  Copy,
  Focus,
  FolderOpen,
  Minus,
  Moon,
  PanelBottom,
  PanelLeft,
  Square as SquareIcon,
  Sun,
  X,
} from "lucide-react";

export default function TopBar() {
  const t = useT();
  const toggleLocale = useLocaleStore((s) => s.toggleLocale);
  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const ideMode = useAgentStore((s) => s.ideMode);
  const changeMode = useAgentStore((s) => s.changeMode);
  const setIdeMode = useAgentStore((s) => s.setIdeMode);
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
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const activeTab = openFiles.find((file) => file.path === activeFile) ?? null;
  const lspStatus = useLspStore((s) => s.status);
  const lspMessage = useLspStore((s) => s.message);
  const diagnosticSummaries = useLspStore((s) => s.diagnosticSummaries);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const { tasks } = useProjectTasks();
  const runProjectTask = useRunProjectTask();

  const [isMaximized, setIsMaximized] = useState(false);
  const [lspDetailsOpen, setLspDetailsOpen] = useState(false);
  const [lspDetails, setLspDetails] = useState<LspStatusSnapshot | null>(null);

  const isRunning = agentRunIsLive(agentState);
  // 「等你回答」不是忙：那时 Agent 停着等人，模式当然可以换。恢复一次被中断的会话也
  // 落在这个状态上，把它算成忙会让这两个按钮再也点不动（见 `isAgentBusy`）。
  const settingsLocked = isAgentBusy(agentState);

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
    (mode: AgentMode) => {
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

  const handleCommandPalette = useCallback(() => {
    window.dispatchEvent(new CustomEvent("toggle-command-palette"));
  }, []);

  const handleLspStatusClick = useCallback(async () => {
    setLspDetailsOpen((value) => !value);
    const languageId = activeTab?.language ?? languageFromPath(activeTab?.path ?? "");
    const snapshot = (await getLspStatus()) ?? (await probeLsp(workspacePath || null, languageId));
    if (snapshot?.status === "unavailable") {
      const probe = await probeLsp(workspacePath || null, languageId);
      setLspDetails(probe ?? snapshot);
      return;
    }
    setLspDetails(snapshot);
  }, [activeTab, workspacePath]);

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
        title: t("topbar.openWorkspaceDialog"),
      });
      if (selected && typeof selected === "string") {
        await invoke("save_workspace_path", { path: selected });
        setWorkspacePath(selected);
        useEditorStore.getState().setWorkspacePath(selected);
      }
    } catch (e) {
      console.warn("Open folder failed:", e);
    }
  }, [setWorkspacePath, t]);

  // 提取项目名
  const projectName = workspacePath
    ? workspacePath.split(/[/\\]/).pop() || workspacePath
    : t("topbar.noFolder");
  const runTask = pickTask(tasks, ["dev", "start", "run"]);
  const debugTask = pickTask(tasks, ["debug", "preview"]);
  const buildTask = pickTask(tasks, ["build"]);
  const testTask = pickTask(tasks, ["test"]);
  const buildStatus = buildTask ? taskRuns[buildTask.id]?.status : undefined;
  const testStatus = testTask ? taskRuns[testTask.id]?.status : undefined;

  return (
    <div
      data-testid="topbar"
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
          title={t("topbar.openFolder")}
        >
          <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
        <span
          className="text-xs text-surface-muted truncate max-w-[200px]"
          title={workspacePath}
        >
          {projectName}
        </span>
      </div>

      {/* 中间：项目命令 + Agent 模式切换 */}
      <div className="relative flex items-center gap-2">
        <button
          type="button"
          onClick={handleLspStatusClick}
          className={`rounded border px-1.5 py-0.5 text-[10px] ${lspStatusClass(lspStatus)}`}
          title={lspMessage ?? t(`lsp.status.${lspStatus}`)}
        >
          {lspBadgeLabel(lspDetails?.languageId ?? activeTab?.language ?? languageFromPath(activeTab?.path ?? ""))} {t(`lsp.short.${lspStatus}`)}
        </button>
        {lspDetailsOpen && (
          <div className="absolute top-10 left-1/2 z-50 w-[360px] -translate-x-1/2 rounded border border-surface-border bg-surface-panel p-3 text-[11px] text-surface-text shadow-xl">
            <div className="mb-2 flex items-center justify-between">
              <span className="font-semibold">{lspDetails?.languageName ?? t("lsp.title")}</span>
              <button
                type="button"
                onClick={() => setLspDetailsOpen(false)}
                aria-label={t("lsp.closeDetails")}
                title={t("lsp.close")}
                className="text-surface-muted hover:text-surface-text"
              >
                x
              </button>
            </div>
            {/* 这几行的标签键早就在 `lsp.*` 里了，之前只是没接上 —— 键写好了、界面还写着
                英文字面量。状态那行显示后端原话优先，没有原话就按状态给默认说明。 */}
            <LspDetailRow label={t("lsp.status")} value={lspDetails?.status ?? t(`lsp.short.${lspStatus}`)} />
            <LspDetailRow
              label={t("lsp.message")}
              value={lspDetails?.message ?? lspMessage ?? t(`lsp.status.${lspStatus}`)}
            />
            <LspDetailRow label={t("lsp.server")} value={lspDetails?.serverPath ?? "-"} />
            <LspDetailRow label={t("lsp.source")} value={lspDetails?.serverSource ?? "-"} />
            <LspDetailRow label={t("lsp.workspace")} value={lspDetails?.workspaceRoot ?? (workspacePath || "-")} />
            <LspDetailRow
              label={t("lsp.indexing")}
              value={`${lspDetails?.indexingStatus ?? t("lsp.unknown")} - ${lspDetails?.indexingMessage ?? t("lsp.noIndexingDetails")}`}
            />
            <LspDetailRow
              label={t("lsp.config")}
              value={
                lspDetails?.workspaceConfigFiles?.length
                  ? lspDetails.workspaceConfigFiles.join(", ")
                  : t("lsp.noConfig")
              }
            />
            <div className="mt-2 grid grid-cols-3 gap-2">
              <LspMetric label={t("lsp.opened")} value={lspDetails?.openedDocuments ?? 0} />
              <LspMetric label={t("lsp.changes")} value={lspDetails?.changeCount ?? 0} />
              <LspMetric label={t("lsp.diagnostics")} value={lspDetails?.diagnosticsCount ?? 0} />
            </div>
            {(lspDetails?.status ?? lspStatus) !== "ready" && (
              <div className="mt-2 rounded border border-diff-modify/30 bg-diff-modify/10 p-2">
                <div className="mb-1 text-[10px] uppercase text-surface-muted">{t("lsp.install")}</div>
                <code className="block select-text break-all font-mono text-[10px] text-surface-text">
                  {lspDetails?.installCommand ?? installCommandForLanguage(activeTab?.language ?? languageFromPath(activeTab?.path ?? ""))}
                </code>
              </div>
            )}
            {lspDetails?.lastError && (
              <div className="mt-2 rounded border border-diff-remove/30 bg-diff-remove/10 p-2 text-diff-remove">
                {lspDetails.lastError}
              </div>
            )}
            <div className="mt-3 border-t border-surface-border pt-2">
              <div className="mb-1 text-[10px] uppercase text-surface-muted">
                {t("lsp.recentDiagnostics")}
              </div>
              {diagnosticSummaries.length === 0 ? (
                <div className="text-surface-muted">{t("lsp.noDiagnostics")}</div>
              ) : (
                <div className="max-h-28 space-y-1 overflow-auto">
                  {diagnosticSummaries.map((summary) => (
                    <div
                      key={summary.file}
                      className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 rounded bg-surface-base px-2 py-1"
                    >
                      <span className="truncate font-mono" title={summary.file}>
                        {summary.file}
                      </span>
                      <span className="font-mono text-[10px]">
                        <span className="text-diff-remove">{summary.error}</span>
                        <span className="text-surface-muted">/</span>
                        <span className="text-diff-modify">{summary.warning}</span>
                        <span className="text-surface-muted">/</span>
                        <span className="text-accent-blue">{summary.info}</span>
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        <div className="flex items-center gap-1 rounded border border-surface-border bg-surface-base px-1 py-0.5">
          <button
            onClick={() => runProjectTask(runTask)}
            disabled={!runTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-text hover:bg-surface-border/40 disabled:cursor-not-allowed disabled:opacity-40"
            title={runTask ? runTask.command : t("topbar.noTask", { kind: t("topbar.run") })}
          >
            {t("topbar.run")}
          </button>
          <button
            onClick={() => runProjectTask(debugTask)}
            disabled={!debugTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-text hover:bg-surface-border/40 disabled:cursor-not-allowed disabled:opacity-40"
            title={debugTask ? debugTask.command : t("topbar.noTask", { kind: t("topbar.debug") })}
          >
            {t("topbar.debug")}
          </button>
          <button
            onClick={() => runProjectTask(buildTask)}
            disabled={!buildTask || !isTauriRuntime()}
            className="rounded px-2 py-0.5 text-[11px] text-surface-muted hover:bg-surface-border/40 hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
            title={buildTask ? buildTask.command : t("topbar.noTask", { kind: t("topbar.build") })}
          >
            {buildStatus === "running" ? t("topbar.building") : t("topbar.build")}
          </button>
          <button
          onClick={() => runProjectTask(testTask)}
          disabled={!testTask || !isTauriRuntime()}
          data-testid="topbar-test"
          className="rounded px-2 py-0.5 text-[11px] text-surface-muted hover:bg-surface-border/40 hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
            title={testTask ? testTask.command : t("topbar.noTask", { kind: t("topbar.test") })}
          >
            {testStatus === "running" ? t("topbar.testing") : t("topbar.test")}
          </button>
        </div>
        <div className="flex items-center gap-0.5 rounded border border-surface-border bg-surface-base p-0.5">
          {(["code", "plan"] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => setIdeMode(mode)}
              disabled={settingsLocked}
              className={`rounded px-2 py-0.5 text-[11px] transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                ideMode === mode
                  ? "bg-accent-blue text-white"
                  : "text-surface-muted hover:bg-surface-border/40 hover:text-surface-text"
              }`}
              title={mode === "plan" ? t("topbar.ideMode.plan.title") : t("topbar.ideMode.code.title")}
            >
              {mode === "plan" ? t("topbar.ideMode.plan") : t("topbar.ideMode.code")}
            </button>
          ))}
        </div>
        <ModeSwitch mode={agentMode} onChange={handleModeChange} />
      </div>

      {/* 右侧：控制按钮 + 窗口控件。被动状态在底部状态栏里 */}
      <div className="flex items-center gap-2">
        {isRunning && (
          <button
            onClick={handleStop}
            className="inline-flex items-center gap-1 px-2.5 py-1 text-xs bg-red-600/70 hover:bg-red-600 text-white rounded transition-colors"
            title={t("topbar.stop.title")}
          >
            <SquareIcon aria-hidden="true" className="h-3 w-3 fill-current" />
            {t("topbar.stop")}
          </button>
        )}

        <div className="w-px h-4 bg-surface-border" />

        {/* 面板切换按钮组 */}
        <button
          onClick={toggleLeftPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={t("topbar.toggleExplorer")}
        >
          <PanelLeft aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={toggleRightPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={t("topbar.toggleAgent")}
        >
          <Bot aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={toggleBottomPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={t("topbar.toggleTerminal")}
        >
          <PanelBottom aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={toggleFocusMode}
          className={`text-xs transition-colors p-0.5 ${
            focusMode ? "text-accent-purple" : "text-surface-muted hover:text-surface-text"
          }`}
          title={t("topbar.focusMode")}
        >
          <Focus aria-hidden="true" className="h-3.5 w-3.5" />
        </button>

        <div className="w-px h-4 bg-surface-border" />

        {/* 语言切换。放在主题旁边而不是设置里：一个中文用户最先要找的就是这个，
            而它藏在设置面板第几屏都算藏。 */}
        <button
          onClick={toggleLocale}
          className="rounded px-1 text-[11px] text-surface-muted transition-colors hover:bg-surface-border/30 hover:text-surface-text"
          title={t("language.toggle")}
        >
          {t("language.short")}
        </button>

        {/* 主题切换 */}
        <button
          onClick={toggleTheme}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={theme === "dark" ? t("topbar.theme.toLight") : t("topbar.theme.toDark")}
        >
          {theme === "dark" ? (
            <Sun aria-hidden="true" className="h-3.5 w-3.5" />
          ) : (
            <Moon aria-hidden="true" className="h-3.5 w-3.5" />
          )}
        </button>

        {/* 命令面板 */}
        <button
          onClick={handleCommandPalette}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={t("topbar.commandPalette")}
        >
          <Command aria-hidden="true" className="h-3.5 w-3.5" />
        </button>

        {/* 快捷键帮助 */}
        <button
          onClick={handleHelp}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors p-0.5"
          title={t("topbar.shortcuts")}
        >
          <CircleHelp aria-hidden="true" className="h-3.5 w-3.5" />
        </button>

        {/* 窗口控制按钮 */}
        <div className="flex items-center ml-1" data-tauri-drag-region="false">
          <button
            onClick={handleMinimize}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-surface-text hover:bg-surface-border/30 transition-colors text-sm"
            title={t("topbar.minimize")}
          >
            <Minus aria-hidden="true" className="h-3.5 w-3.5" />
          </button>
          <button
            onClick={handleMaximize}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-surface-text hover:bg-surface-border/30 transition-colors text-sm"
            title={isMaximized ? t("topbar.restore") : t("topbar.maximize")}
          >
            {isMaximized ? (
              <Copy aria-hidden="true" className="h-3.5 w-3.5" />
            ) : (
              <SquareIcon aria-hidden="true" className="h-3.5 w-3.5" />
            )}
          </button>
          <button
            onClick={handleClose}
            className="w-8 h-8 flex items-center justify-center text-surface-muted hover:text-white hover:bg-red-600 transition-colors text-sm"
            title={t("topbar.close")}
          >
            <X aria-hidden="true" className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

function LspDetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[72px_minmax(0,1fr)] gap-2 py-0.5">
      <span className="text-surface-muted">{label}</span>
      <span className="truncate font-mono" title={value}>
        {value}
      </span>
    </div>
  );
}

function LspMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded border border-surface-border bg-surface-base px-2 py-1">
      <div className="text-[10px] uppercase text-surface-muted">{label}</div>
      <div className="font-mono text-surface-text">{value}</div>
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

function lspStatusClass(status: string) {
  switch (status) {
    case "ready":
      return "border-green-500/40 bg-green-500/10 text-green-300";
    case "checking":
      return "border-accent-blue/40 bg-accent-blue/10 text-accent-blue";
    case "unavailable":
    case "error":
      return "border-amber-500/50 bg-amber-500/10 text-amber-300";
    default:
      return "border-surface-border bg-surface-base text-surface-muted";
  }
}

function languageFromPath(path: string) {
  const ext = path.split(".").pop()?.toLowerCase();
  if (ext === "go") return "go";
  if (ext === "py") return "python";
  if (ext === "rs") return "rust";
  if (ext === "ts" || ext === "tsx") return "typescript";
  if (ext === "js" || ext === "jsx") return "javascript";
  return "typescript";
}

function lspBadgeLabel(languageId: string) {
  if (languageId === "go") return "Go";
  if (languageId === "python") return "Py";
  if (languageId === "rust") return "Rust";
  return "TS";
}

function installCommandForLanguage(languageId: string) {
  if (languageId === "go") return "go install golang.org/x/tools/gopls@latest";
  if (languageId === "python") return "npm install -D pyright";
  if (languageId === "rust") return "rustup component add rust-analyzer";
  return "npm install -D typescript typescript-language-server";
}
