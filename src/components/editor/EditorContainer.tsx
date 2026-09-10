import { Suspense, lazy, useEffect, useCallback, useState, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLspStore } from "../../stores/useLspStore";
import { useThemeStore } from "../../stores/useThemeStore";
import { pathsEqual } from "../../utils/paths";
import { MonacoContext } from "./MonacoContext";
import {
  registerGlobalMonacoFeatures,
  runAgentSelectionAction,
  setCurrentEditor,
} from "./monacoGlobals";
import {
  AGENT_QUICK_ACTIONS,
  type AgentQuickActionKey,
} from "../../utils/agentActions";
import EditorTabs from "./EditorTabs";
import InlineSuggestion from "./InlineSuggestion";
import DiffOverlay from "./DiffOverlay";
import IntentHint from "./IntentHint";
import QuickActions from "./QuickActions";
import DiagnosticsBridge from "./DiagnosticsBridge";
import ProblemsMarkerBridge from "./ProblemsMarkerBridge";
import {
  configureTypeScriptSemantic,
  ensureOpenFileModels,
} from "../../utils/typescriptSemantic";
import { useLspDiagnostics } from "../../hooks/useLspDiagnostics";
import { useIncrementalRendering } from "../../hooks/useIncrementalRendering";
import PerformanceMetricsPanel from "./PerformanceMetricsPanel";
import {
  changeLspFile,
  initializeLsp,
  isLspLanguage,
  openLspFile,
} from "../../utils/lspClient";


import type { editor } from "monaco-editor";

const MonacoEditor = lazy(() => import("@monaco-editor/react"));

/** 简单语言 detector */
function detectLanguage(path: string): string {
  const ext = path.split(".").pop() || "txt";
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    json: "json",
    css: "css",
    html: "html",
    md: "markdown",
    rs: "rust",
    go: "go",
    py: "python",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
  };
  return map[ext] || "plaintext";
}

/** 默认欢迎页 */
const WELCOME_CODE = `//  Welcome to Agent IDE
//  🧠 AI-Powered Development Environment
//
//  Try:
//    • Select code → Quick Actions (Explain / Fix / Refactor)
//    • Chat with Agent in the right panel
//    • Drag files into Agent context
//
//  Mode: Suggest | Auto
`;

export default function EditorContainer() {
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const setLspStatus = useLspStore((s) => s.setStatus);
  const updateFileContent = useEditorStore((s) => s.updateFileContent);
  const saveCurrentFile = useEditorStore((s) => s.saveCurrentFile);
  const saveError = useEditorStore((s) => s.saveError);
  const clearSaveError = useEditorStore((s) => s.clearSaveError);
  const setSelectedText = useEditorStore((s) => s.setSelectedText);
  const setSelectedRange = useEditorStore((s) => s.setSelectedRange);
  const setCursorPosition = useEditorStore((s) => s.setCursorPosition);
  const pendingRevealLocation = useEditorStore((s) => s.pendingRevealLocation);
  const clearPendingRevealLocation = useEditorStore((s) => s.clearPendingRevealLocation);

  // Agent / 右侧面板的状态刻意不订阅：只有右键菜单那几个回调用得到，而它们在
  // `monacoGlobals` 里一律 `getState()` 现取。订阅了反而让整个编辑器容器跟着
  // Agent 每次状态变化和每次面板开合重渲染一遍。
  const performanceOverlay = useLayoutStore((s) => s.performanceOverlay);
  const togglePerformanceOverlay = useLayoutStore((s) => s.togglePerformanceOverlay);
  const theme = useThemeStore((s) => s.theme);

  const [editorRef, setEditorRef] = useState<editor.IStandaloneCodeEditor | null>(null);
  const [monacoRef, setMonacoRef] = useState<typeof import("monaco-editor") | null>(null);
  useLspDiagnostics(monacoRef);
  const editorContainerRef = useRef<HTMLDivElement>(null);
  const disposablesRef = useRef<Set<{ dispose(): void }>>(new Set());
  const lspOpenedFilesRef = useRef<Set<string>>(new Set());
  const lspFileVersionsRef = useRef<Map<string, number>>(new Map());
  const lspChangeTimerRef = useRef<number | null>(null);
  const [lspReady, setLspReady] = useState(false);

  // 集成增量渲染引擎。仅在性能浮层打开时启用：renderLine 是刻意的空操作
  // （Monaco 自己负责渲染），所以这个循环唯一的产出就是指标，关掉浮层还继续跑
  // 等于每帧白白 setState 一次。
  const {
    metrics: renderMetrics,
    resetMetrics: resetRenderMetrics,
  } = useIncrementalRendering(editorRef, {
    enabled: performanceOverlay,
    config: {
      frameBudgetMs: 16, // 60fps 目标
      targetFps: 60,
      enableMultiThreading: false, // 目前不启用多线程
      dirtyLineTtl: 1000,
      maxRenderQueueSize: 1000,
    },
    profilingEnabled: performanceOverlay,
  });

  const activeTab = openFiles.find((f) => f.path === activeFile);
  const currentContent = activeFile ? fileContents[activeFile] ?? "" : "";

  // Ctrl+S 保存
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        saveCurrentFile();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [saveCurrentFile]);

  // 组件卸载时清理所有 Monaco disposable
  useEffect(() => {
    return () => {
      disposablesRef.current.forEach((d) => d.dispose());
      disposablesRef.current.clear();
    };
  }, []);

  // 关掉最后一个 tab 时 `<MonacoEditor>` 整体卸载，编辑器实例被 Monaco 释放 ——
  // 但灯泡 provider 和 apply-code-action 注册在 monaco 模块上，活到页面结束。所以
  // 这里必须主动把"当前编辑器"清空，否则它们会拿一个已释放的实例去调
  // `getSelection()` / `getModel()`；`MonacoContext` 也一样，消费者会照着一个死
  // 实例算坐标。
  const hasActiveTab = Boolean(activeTab);
  useEffect(() => {
    if (hasActiveTab) return;
    setCurrentEditor(null);
    setEditorRef(null);
  }, [hasActiveTab]);

  const handleChange = useCallback(
    (value: string | undefined) => {
      if (activeFile && value !== undefined) {
        updateFileContent(activeFile, value);
      }
    },
    [activeFile, updateFileContent]
  );

  // Monaco onMount: capture editor + monaco, register selection listener
  const handleEditorMount = useCallback(
    (editorInst: editor.IStandaloneCodeEditor, monacoInst: typeof import("monaco-editor")) => {
      setEditorRef(editorInst);
      setMonacoRef(monacoInst);
      // 模块级的 provider / command 只能通过这里知道"当前是哪个编辑器"
      setCurrentEditor(editorInst);
      configureTypeScriptSemantic(monacoInst);
      // 幂等：按 monaco 模块去重，`key={activeFile}` 造成的重复挂载不会重复注册
      registerGlobalMonacoFeatures(monacoInst);

      // 选区变化 → 更新 store
      const selectionDisposable = editorInst.onDidChangeCursorSelection(() => {
        const selection = editorInst.getSelection();
        if (selection && !selection.isEmpty()) {
          const model = editorInst.getModel();
          if (model) {
            const text = model.getValueInRange(selection);
            setSelectedText(text);
            setSelectedRange({
              startLine: selection.startLineNumber,
              endLine: selection.endLineNumber,
            });
          }
        } else {
          setSelectedText(null);
          setSelectedRange(null);
        }
      });
      disposablesRef.current.add(selectionDisposable);

      // 光标位置单独监听：选区监听在取消选择时会把状态清空，而"我在第几行"
      // 是一直成立的事实，状态栏要一直显示得出来
      const cursorDisposable = editorInst.onDidChangeCursorPosition((event) => {
        setCursorPosition({ line: event.position.lineNumber, column: event.position.column });
      });
      disposablesRef.current.add(cursorDisposable);
      const initialPosition = editorInst.getPosition();
      if (initialPosition) {
        setCursorPosition({
          line: initialPosition.lineNumber,
          column: initialPosition.column,
        });
      }

      const definitionDisposable = editorInst.addAction({
        id: "agent-ide.go-to-definition",
        label: "Go to Definition",
        keybindings: [monacoInst.KeyCode.F12],
        contextMenuGroupId: "navigation",
        contextMenuOrder: 1,
        run: async (ed) => {
          await ed.getAction("editor.action.revealDefinition")?.run();
        },
      });
      disposablesRef.current.add(definitionDisposable);

      // 右键菜单项是 `editorInst.addAction`，属于这个编辑器实例，所以每次挂载都要
      // 重新加一遍（也就随实例一起被释放）。动作本体在 monacoGlobals 里，和灯泡
      // 命令共用同一个函数，两条入口的忙判断和上下文取值不会走岔。
      for (const act of AGENT_QUICK_ACTIONS) {
        const disposable = editorInst.addAction({
          id: `agent-ide.${act.key}-selection`,
          label: `${act.icon} ${act.label} with Agent`,
          contextMenuGroupId: "agent",
          contextMenuOrder: AGENT_QUICK_ACTIONS.indexOf(act) + 1,
          precondition: "editorHasSelection",
          run: () => {
            void runAgentSelectionAction(act.key as AgentQuickActionKey);
          },
        });
        disposablesRef.current.add(disposable);
      }
    },
    [setCursorPosition, setSelectedRange, setSelectedText]
  );


  const contextValue = { editor: editorRef, monaco: monacoRef };

  useEffect(() => {
    if (!monacoRef) return;
    ensureOpenFileModels(monacoRef, openFiles, fileContents);
  }, [fileContents, monacoRef, openFiles]);

  useEffect(() => {
    let cancelled = false;
    const languageId = activeTab ? activeTab.language || detectLanguage(activeTab.path) : "typescript";
    if (!isLspLanguage(languageId)) {
      setLspReady(false);
      setLspStatus("idle", "Open a TypeScript/JavaScript, Go, Python, or Rust file to start a language server.");
      return;
    }
    lspOpenedFilesRef.current.clear();
    lspFileVersionsRef.current.clear();
    setLspReady(false);
    setLspStatus("checking");

    void initializeLsp(workspacePath || null, languageId).then(({ ready, message }) => {
      if (cancelled) return;
      setLspReady(ready);
      setLspStatus(ready ? "ready" : "unavailable", message);
    });

    return () => {
      cancelled = true;
    };
  }, [activeTab, setLspStatus, workspacePath]);

  useEffect(() => {
    if (!lspReady || !activeFile || !activeTab) return;
    const languageId = activeTab.language || detectLanguage(activeTab.path);
    if (!isLspLanguage(languageId)) return;

    if (lspChangeTimerRef.current !== null) {
      window.clearTimeout(lspChangeTimerRef.current);
      lspChangeTimerRef.current = null;
    }

    if (!lspOpenedFilesRef.current.has(activeFile)) {
      lspOpenedFilesRef.current.add(activeFile);
      lspFileVersionsRef.current.set(activeFile, 1);
      void openLspFile(activeFile, currentContent, languageId, 1).catch((error) => {
        console.warn("Open LSP document failed:", error);
      });
      return;
    }

    lspChangeTimerRef.current = window.setTimeout(() => {
      const nextVersion = (lspFileVersionsRef.current.get(activeFile) ?? 1) + 1;
      lspFileVersionsRef.current.set(activeFile, nextVersion);
      void changeLspFile(activeFile, currentContent, languageId, nextVersion).catch((error) => {
        console.warn("Change LSP document failed:", error);
      });
    }, 250);

    return () => {
      if (lspChangeTimerRef.current !== null) {
        window.clearTimeout(lspChangeTimerRef.current);
        lspChangeTimerRef.current = null;
      }
    };
  }, [activeFile, activeTab, currentContent, lspReady]);

  useEffect(() => {
    if (!editorRef || !monacoRef || !activeFile || !pendingRevealLocation) return;
    if (!pathsEqual(pendingRevealLocation.file, activeFile)) return;

    const position = {
      lineNumber: Math.max(1, pendingRevealLocation.line),
      column: Math.max(1, pendingRevealLocation.column),
    };
    editorRef.setPosition(position);
    editorRef.revealPositionInCenter(position, monacoRef.editor.ScrollType.Smooth);
    editorRef.focus();
    clearPendingRevealLocation();
  }, [activeFile, clearPendingRevealLocation, editorRef, monacoRef, pendingRevealLocation]);

  return (
    <div className="h-full flex flex-col bg-surface-base" ref={editorContainerRef}>
      {/* 文件标签栏 */}
      <EditorTabs />

      {/* 保存失败/被拒必须看得见。以前只走 console.error，用户以为已经存下去了。 */}
      {saveError && (
        <div
          role="alert"
          className="flex flex-shrink-0 items-start gap-2 border-b border-diff-remove/40 bg-diff-remove/10 px-3 py-1.5 text-[11px] text-diff-remove"
        >
          <span className="min-w-0 flex-1 break-words">{saveError}</span>
          <button
            onClick={clearSaveError}
            aria-label="Dismiss save error"
            title="Dismiss"
            className="flex-shrink-0 text-surface-muted hover:text-surface-text"
          >
            ×
          </button>
        </div>
      )}


      {/* Monaco 编辑器区 */}
      <div className="flex-1 relative overflow-hidden">
        <PerformanceMetricsPanel
          metrics={renderMetrics}
          onReset={resetRenderMetrics}
          onClose={togglePerformanceOverlay}
        />
        <MonacoContext.Provider value={contextValue}>
          {activeTab ? (
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-full text-surface-muted text-sm">
                  Loading editor...
                </div>
              }
            >
              <MonacoEditor
                key={activeFile}
                path={activeFile ?? undefined}
                height="100%"
                language={activeTab.language || detectLanguage(activeTab.path)}
                theme={theme === "light" ? "vs" : "vs-dark"}
                value={currentContent}
                onChange={handleChange}
                onMount={handleEditorMount}
                options={{
                  fontSize: 13,
                  fontFamily:
                    "'JetBrains Mono', 'Fira Code', 'Consolas', monospace",
                  minimap: { enabled: true, scale: 1, showSlider: "mouseover" },
                  scrollBeyondLastLine: false,
                  wordWrap: "off",
                  lineNumbers: "on",
                  renderWhitespace: "selection",
                  bracketPairColorization: { enabled: true },
                  automaticLayout: true,
                  tabSize: 2,
                  insertSpaces: true,
                  smoothScrolling: true,
                  cursorBlinking: "smooth",
                  cursorSmoothCaretAnimation: "on",
                  padding: { top: 8 },
                }}
              />

              {/* AI 增强层 */}
              <InlineSuggestion />
              <DiffOverlay />
              <IntentHint />
              <QuickActions />
              <DiagnosticsBridge />
              <ProblemsMarkerBridge />
            </Suspense>
          ) : (
            <div className="h-full flex items-center justify-center">
              <div className="text-center">
                <div className="text-5xl mb-4">🧠</div>
                <h2 className="text-xl font-semibold text-surface-text mb-2">
                  Agent IDE
                </h2>
                <p className="text-sm text-surface-muted max-w-md leading-relaxed">
                  AI-powered development environment.
                  <br />
                  Open a file or start a conversation with your Agent.
                </p>
                <pre className="mt-6 text-left text-xs font-mono text-surface-muted bg-surface-panel p-4 rounded-lg inline-block max-w-lg overflow-auto">
                  {WELCOME_CODE}
                </pre>
              </div>
            </div>
          )}
        </MonacoContext.Provider>
      </div>
    </div>
  );
}


