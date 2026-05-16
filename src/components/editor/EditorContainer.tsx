import { Suspense, lazy, useEffect, useCallback, useState, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLspStore } from "../../stores/useLspStore";
import { useLogStore } from "../../stores/useLogStore";
import { pathsEqual } from "../../utils/paths";
import { MonacoContext } from "./MonacoContext";
import EditorTabs from "./EditorTabs";
import InlineSuggestion from "./InlineSuggestion";
import DiffOverlay from "./DiffOverlay";
import IntentHint from "./IntentHint";
import QuickActions from "./QuickActions";
import DiagnosticsBridge from "./DiagnosticsBridge";
import ProblemsMarkerBridge from "./ProblemsMarkerBridge";
import { buildLocalCompletionCandidates, type CompletionCandidateKind } from "../../utils/codeCompletion";
import {
  configureTypeScriptSemantic,
  ensureOpenFileModels,
} from "../../utils/typescriptSemantic";
import { useLspDiagnostics } from "../../hooks/useLspDiagnostics";
import {
  changeLspFile,
  getLspCodeActions,
  getLspCompletion,
  getLspDefinition,
  getLspDocumentSymbols,
  getLspHover,
  getLspRename,
  initializeLsp,
  isLspLanguage,
  lspRangeToMonacoRange,
  monacoRangeToLspRange,
  openLspFile,
  toMonacoSymbolKind,
  type LspDocumentSymbol,
  type LspDiagnostic,
  type LspWorkspaceEdit,
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
//  Mode: Suggest | Edit | Auto
`;

export default function EditorContainer() {
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const setLspStatus = useLspStore((s) => s.setStatus);
  const addLog = useLogStore((s) => s.addLog);
  const updateFileContent = useEditorStore((s) => s.updateFileContent);
  const saveCurrentFile = useEditorStore((s) => s.saveCurrentFile);
  const setSelectedText = useEditorStore((s) => s.setSelectedText);
  const setSelectedRange = useEditorStore((s) => s.setSelectedRange);
  const pendingRevealLocation = useEditorStore((s) => s.pendingRevealLocation);
  const clearPendingRevealLocation = useEditorStore((s) => s.clearPendingRevealLocation);

  const [editorRef, setEditorRef] = useState<editor.IStandaloneCodeEditor | null>(null);
  const [monacoRef, setMonacoRef] = useState<typeof import("monaco-editor") | null>(null);
  useLspDiagnostics(monacoRef);
  const editorContainerRef = useRef<HTMLDivElement>(null);
  const disposablesRef = useRef<Set<{ dispose(): void }>>(new Set());
  const completionRegisteredRef = useRef(false);
  const lspRegisteredRef = useRef(false);
  const lspOpenedFilesRef = useRef<Set<string>>(new Set());
  const lspFileVersionsRef = useRef<Map<string, number>>(new Map());
  const lspChangeTimerRef = useRef<number | null>(null);
  const [lspReady, setLspReady] = useState(false);

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
      configureTypeScriptSemantic(monacoInst);

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

      if (!completionRegisteredRef.current) {
        completionRegisteredRef.current = true;
        const completionLanguages = [
          "rust",
          "python",
          "css",
          "html",
          "json",
          "markdown",
          "yaml",
          "toml",
        ];
        for (const language of completionLanguages) {
          const completionDisposable = monacoInst.languages.registerCompletionItemProvider(language, {
            triggerCharacters: [".", "/", "\\", "'", "\"", "@", "<"],
            provideCompletionItems: (model, position) => {
              const word = model.getWordUntilPosition(position);
              const currentWord = word.word;
              const range = {
                startLineNumber: position.lineNumber,
                endLineNumber: position.lineNumber,
                startColumn: word.startColumn,
                endColumn: word.endColumn,
              };
              const editorState = useEditorStore.getState();
              const candidates = buildLocalCompletionCandidates({
                content: model.getValue(),
                language: model.getLanguageId(),
                currentWord,
                linePrefix: model.getLineContent(position.lineNumber).slice(0, position.column - 1),
                openFilePaths: editorState.openFiles.map((file) => file.path),
              });

              return {
                suggestions: candidates.map((candidate) => ({
                  label: candidate.label,
                  kind: toMonacoCompletionKind(monacoInst, candidate.kind),
                  insertText: candidate.insertText,
                  insertTextRules:
                    candidate.kind === "snippet"
                      ? monacoInst.languages.CompletionItemInsertTextRule.InsertAsSnippet
                      : undefined,
                  detail: candidate.detail,
                  sortText: `${999 - candidate.score}-${candidate.label}`,
                  range,
                })),
              };
            },
          });
          disposablesRef.current.add(completionDisposable);
        }
      }

      if (!lspRegisteredRef.current) {
        lspRegisteredRef.current = true;
        for (const language of ["typescript", "javascript"]) {
          const completionDisposable = monacoInst.languages.registerCompletionItemProvider(language, {
            triggerCharacters: [".", "\"", "'", "/", "@", "<"],
            provideCompletionItems: async (model, position) => {
              const word = model.getWordUntilPosition(position);
              const file = model.uri.fsPath || model.uri.path;
              const items = await getLspCompletion(file, position.lineNumber - 1, position.column - 1);
              return {
                suggestions: items.map((item) => ({
                  label: item.label,
                  kind: toMonacoCompletionItemKind(monacoInst, item.kind),
                  insertText: item.insertText || item.label,
                  detail: item.detail,
                  documentation: item.documentation ? { value: item.documentation } : undefined,
                  sortText: item.sortText,
                  filterText: item.filterText,
                  range: {
                    startLineNumber: position.lineNumber,
                    endLineNumber: position.lineNumber,
                    startColumn: word.startColumn,
                    endColumn: word.endColumn,
                  },
                })),
              };
            },
          });
          disposablesRef.current.add(completionDisposable);

          const hoverDisposable = monacoInst.languages.registerHoverProvider(language, {
            provideHover: async (model, position) => {
              const file = model.uri.fsPath || model.uri.path;
              const hover = await getLspHover(file, position.lineNumber - 1, position.column - 1);
              if (!hover?.contents) return null;
              return {
                contents: [{ value: hover.contents }],
                range: hover.range ? lspRangeToMonacoRange(hover.range) : undefined,
              };
            },
          });
          disposablesRef.current.add(hoverDisposable);

          const definitionProviderDisposable = monacoInst.languages.registerDefinitionProvider(language, {
            provideDefinition: async (model, position) => {
              const file = model.uri.fsPath || model.uri.path;
              const locations = await getLspDefinition(file, position.lineNumber - 1, position.column - 1);
              return locations.map((location) => ({
                uri: monacoInst.Uri.file(location.file),
                range: lspRangeToMonacoRange(location.range),
              }));
            },
          });
          disposablesRef.current.add(definitionProviderDisposable);

          const documentSymbolDisposable = monacoInst.languages.registerDocumentSymbolProvider(language, {
            provideDocumentSymbols: async (model) => {
              const file = model.uri.fsPath || model.uri.path;
              const symbols = await getLspDocumentSymbols(file);
              return flattenDocumentSymbols(monacoInst, symbols);
            },
          });
          disposablesRef.current.add(documentSymbolDisposable);

          const renameDisposable = monacoInst.languages.registerRenameProvider(language, {
            provideRenameEdits: async (model, position, newName) => {
              const file = model.uri.fsPath || model.uri.path;
              const edit = await getLspRename(file, position.lineNumber - 1, position.column - 1, newName);
              if (!edit) {
                return { edits: [] };
              }
              return workspaceEditToMonaco(monacoInst, edit);
            },
          });
          disposablesRef.current.add(renameDisposable);

          const codeActionDisposable = monacoInst.languages.registerCodeActionProvider(language, {
            provideCodeActions: async (model, range) => {
              const file = model.uri.fsPath || model.uri.path;
              const diagnostics = markersToLspDiagnostics(
                monacoInst,
                file,
                monacoInst.editor.getModelMarkers({ resource: model.uri }).filter((marker) =>
                  markerIntersectsRange(marker, range)
                )
              );
              const actions = await getLspCodeActions(file, monacoRangeToLspRange(range), diagnostics);
              return {
                actions: actions
                  .filter((action) => action.edit?.edits.length)
                  .map((action) => ({
                    title: action.title,
                    kind: action.kind ? action.kind.replace(/\./g, ".") : "quickfix",
                    command: {
                      id: "agent-ide.apply-code-action",
                      title: action.title,
                      arguments: [action.title, action.edit!],
                    },
                  })),
                dispose: () => {},
              };
            },
          });
          disposablesRef.current.add(codeActionDisposable);

          const applyCodeActionDisposable = monacoInst.editor.registerCommand(
            "agent-ide.apply-code-action",
            async (_accessor, title: string, edit: LspWorkspaceEdit) => {
              try {
                const applied = applyWorkspaceEdit(editorInst, monacoInst, edit);
                if (!applied) {
                  addLog({
                    time: new Date().toLocaleTimeString(),
                    level: "error",
                    source: "system",
                    message: `Code action failed: ${title}`,
                    details: "Monaco rejected the workspace edit.",
                  });
                  return;
                }
                syncWorkspaceEditToStore(monacoInst, edit, updateFileContent);
                await syncWorkspaceEditToLsp(monacoInst, edit);
                addLog({
                  time: new Date().toLocaleTimeString(),
                  level: "success",
                  source: "system",
                  message: `Code action applied: ${title}`,
                  details: `${edit.edits.length} edit(s) applied.`,
                });
              } catch (error) {
                addLog({
                  time: new Date().toLocaleTimeString(),
                  level: "error",
                  source: "system",
                  message: `Code action failed: ${title}`,
                  details: String(error),
                });
              }
            }
          );
          disposablesRef.current.add(applyCodeActionDisposable);
        }
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
    },
    [addLog, setSelectedRange, setSelectedText, updateFileContent]
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
      setLspStatus("idle", "Open a TypeScript/JavaScript or Go file to start a language server.");
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
                path={activeFile ?? undefined}
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

function toMonacoCompletionKind(
  monaco: typeof import("monaco-editor"),
  kind: CompletionCandidateKind
) {
  switch (kind) {
    case "keyword":
      return monaco.languages.CompletionItemKind.Keyword;
    case "file":
      return monaco.languages.CompletionItemKind.File;
    case "snippet":
      return monaco.languages.CompletionItemKind.Snippet;
    case "symbol":
    default:
      return monaco.languages.CompletionItemKind.Variable;
  }
}

function toMonacoCompletionItemKind(monaco: typeof import("monaco-editor"), kind?: number) {
  const itemKind = monaco.languages.CompletionItemKind;
  const mapping: Record<number, number> = {
    1: itemKind.Text,
    2: itemKind.Method,
    3: itemKind.Function,
    4: itemKind.Constructor,
    5: itemKind.Field,
    6: itemKind.Variable,
    7: itemKind.Class,
    8: itemKind.Interface,
    9: itemKind.Module,
    10: itemKind.Property,
    11: itemKind.Unit,
    12: itemKind.Value,
    13: itemKind.Enum,
    14: itemKind.Keyword,
    15: itemKind.Snippet,
    16: itemKind.Color,
    17: itemKind.File,
    18: itemKind.Reference,
    21: itemKind.Constant,
    22: itemKind.Struct,
    23: itemKind.Event,
    24: itemKind.Operator,
    25: itemKind.TypeParameter,
  };
  return kind ? mapping[kind] ?? itemKind.Variable : itemKind.Variable;
}

function flattenDocumentSymbols(
  monaco: typeof import("monaco-editor"),
  symbols: LspDocumentSymbol[],
  containerName?: string
): import("monaco-editor").languages.DocumentSymbol[] {
  return symbols.flatMap((symbol) => [
    {
      name: symbol.name,
      detail: "",
      kind: toMonacoSymbolKind(monaco, symbol.kind),
      tags: [],
      containerName,
      range: lspRangeToMonacoRange(symbol.range),
      selectionRange: lspRangeToMonacoRange(symbol.selectionRange),
    },
    ...flattenDocumentSymbols(monaco, symbol.children, symbol.name),
  ]);
}

function workspaceEditToMonaco(
  monaco: typeof import("monaco-editor"),
  edit: LspWorkspaceEdit
): import("monaco-editor").languages.WorkspaceEdit {
  return {
    edits: edit.edits.map((textEdit) => ({
      resource: monaco.Uri.file(textEdit.file),
      versionId: undefined,
      textEdit: {
        range: lspRangeToMonacoRange(textEdit.range),
        text: textEdit.newText,
      },
    })),
  };
}

function applyWorkspaceEdit(
  editor: import("monaco-editor").editor.IStandaloneCodeEditor,
  monaco: typeof import("monaco-editor"),
  edit: LspWorkspaceEdit
) {
  const activeModel = editor.getModel();
  if (!activeModel) return false;
  const activeFile = activeModel.uri.fsPath || activeModel.uri.path;
  const activeFileEdits = edit.edits.filter((textEdit) => pathsEqual(textEdit.file, activeFile));
  const otherFileEdits = edit.edits.filter((textEdit) => !pathsEqual(textEdit.file, activeFile));

  const activeApplied = activeFileEdits.length
    ? editor.executeEdits(
        "agent-ide-code-action",
        activeFileEdits.map((textEdit) => ({
          range: lspRangeToMonacoRange(textEdit.range),
          text: textEdit.newText,
        }))
      )
    : true;

  if (!activeApplied) return false;

  for (const textEdit of otherFileEdits) {
    const model = findModelForFile(monaco, textEdit.file);
    if (!model) return false;
    model.applyEdits([
      {
        range: lspRangeToMonacoRange(textEdit.range),
        text: textEdit.newText,
      },
    ]);
  }
  return true;
}

function syncWorkspaceEditToStore(
  monaco: typeof import("monaco-editor"),
  edit: LspWorkspaceEdit,
  updateFileContent: (path: string, content: string) => void
) {
  const touchedFiles = new Set(edit.edits.map((textEdit) => textEdit.file));
  for (const file of touchedFiles) {
    const model = findModelForFile(monaco, file);
    if (model) updateFileContent(file, model.getValue());
  }
}

async function syncWorkspaceEditToLsp(
  monaco: typeof import("monaco-editor"),
  edit: LspWorkspaceEdit
) {
  const touchedFiles = new Set(edit.edits.map((textEdit) => textEdit.file));
  await Promise.all(
    [...touchedFiles].map(async (file) => {
      const model = findModelForFile(monaco, file);
      if (!model || !isLspLanguage(model.getLanguageId())) return;
      await changeLspFile(file, model.getValue(), model.getLanguageId(), model.getVersionId());
    })
  );
}

function findModelForFile(monaco: typeof import("monaco-editor"), file: string) {
  return (
    monaco.editor.getModel(monaco.Uri.file(file)) ??
    monaco.editor
      .getModels()
      .find((model) => pathsEqual(model.uri.fsPath || model.uri.path, file))
  );
}

function markersToLspDiagnostics(
  monaco: typeof import("monaco-editor"),
  file: string,
  markers: import("monaco-editor").editor.IMarker[]
): LspDiagnostic[] {
  return markers.map((marker) => ({
    file,
    range: monacoRangeToLspRange(marker),
    severity: markerSeverityToLsp(monaco, marker.severity),
    message: marker.message,
    source: marker.source,
  }));
}

function markerSeverityToLsp(
  monaco: typeof import("monaco-editor"),
  severity: import("monaco-editor").MarkerSeverity
): LspDiagnostic["severity"] {
  if (severity === monaco.MarkerSeverity.Error) return "error";
  if (severity === monaco.MarkerSeverity.Warning) return "warning";
  return "info";
}

function markerIntersectsRange(
  marker: import("monaco-editor").editor.IMarker,
  range: import("monaco-editor").IRange
) {
  return !(
    marker.endLineNumber < range.startLineNumber ||
    marker.startLineNumber > range.endLineNumber ||
    (marker.endLineNumber === range.startLineNumber && marker.endColumn < range.startColumn) ||
    (marker.startLineNumber === range.endLineNumber && marker.startColumn > range.endColumn)
  );
}
