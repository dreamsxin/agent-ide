import { Suspense, lazy, useEffect, useCallback, useState, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { MonacoContext } from "./MonacoContext";
import EditorTabs from "./EditorTabs";
import InlineSuggestion from "./InlineSuggestion";
import DiffOverlay from "./DiffOverlay";
import IntentHint from "./IntentHint";
import QuickActions from "./QuickActions";

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
//  Mode: Suggest | Edit | Auto
`;

export default function EditorContainer() {
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const updateFileContent = useEditorStore((s) => s.updateFileContent);
  const saveCurrentFile = useEditorStore((s) => s.saveCurrentFile);
  const setSelectedText = useEditorStore((s) => s.setSelectedText);
  const setSelectedRange = useEditorStore((s) => s.setSelectedRange);

  const [editorRef, setEditorRef] = useState<editor.IStandaloneCodeEditor | null>(null);
  const [monacoRef, setMonacoRef] = useState<typeof import("monaco-editor") | null>(null);
  const editorContainerRef = useRef<HTMLDivElement>(null);
  const disposablesRef = useRef<Set<{ dispose(): void }>>(new Set());

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
    },
    [setSelectedText, setSelectedRange]
  );

  const contextValue = { editor: editorRef, monaco: monacoRef };

  return (
    <div className="h-full flex flex-col bg-surface-base" ref={editorContainerRef}>
      {/* 文件标签栏 */}
      <EditorTabs />

      {/* Monaco 编辑器区 */}
      <div className="flex-1 relative overflow-hidden">
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
                height="100%"
                language={activeTab.language || detectLanguage(activeTab.path)}
                theme="vs-dark"
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
