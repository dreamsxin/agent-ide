/**
 * Monaco 里所有**模块级**的注册都集中在这里。
 *
 * 为什么不放在 `EditorContainer` 的 `onMount`：`monaco.languages.register*` 和
 * `monaco.editor.registerCommand` 挂在 monaco 模块上，不属于某个编辑器实例，也
 * 不属于某个 React 组件。放在组件里就要用 ref 做"只注册一次"的守卫，而 ref 的
 * 生命周期和被守卫的资源不是一回事，于是有两个坑：
 *
 *  1. 组件卸载时清掉了注册，守卫 ref 却可能还活着（StrictMode 的模拟重挂载就是
 *     这样：effect 重跑，ref 保留），补偿性重注册永远不会发生 —— 所有 provider
 *     静默消失。
 *  2. 一旦出现第二个编辑器容器（分屏），"每个组件一次"就变成了同一份全局资源
 *     注册两次。
 *
 * 这里改成按 monaco 模块本身去重（`WeakSet`），资源和守卫的生命周期就对齐了。
 * 这些注册**不需要**被 dispose：monaco 模块活到页面结束。
 *
 * 代价是它们没有 React 闭包可用，所以一律 `getState()` 现取 —— 这本来就是正确
 * 做法，注册发生一次，调用发生在很久以后的任意时刻。唯一需要外部告知的事实是
 * "当前是哪个编辑器"，由 `setCurrentEditor` 维护。
 */
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLogStore } from "../../stores/useLogStore";
import { pathsEqual } from "../../utils/paths";
import {
  AGENT_QUICK_ACTIONS,
  buildActionPrompt,
  type AgentQuickActionKey,
} from "../../utils/agentActions";
import {
  buildLocalCompletionCandidates,
  type CompletionCandidateKind,
} from "../../utils/codeCompletion";
import {
  changeLspFile,
  getLspCodeActions,
  getLspCompletion,
  getLspDefinition,
  getLspDocumentSymbols,
  getLspHover,
  getLspRename,
  isLspLanguage,
  lspRangeToMonacoRange,
  monacoRangeToLspRange,
  toMonacoSymbolKind,
  type LspDiagnostic,
  type LspDocumentSymbol,
  type LspWorkspaceEdit,
} from "../../utils/lspClient";

import type { editor } from "monaco-editor";

type Monaco = typeof import("monaco-editor");

/** 本地补全（没有 LSP 的语言）注册的语言 */
const LOCAL_COMPLETION_LANGUAGES = [
  "rust",
  "python",
  "css",
  "html",
  "json",
  "markdown",
  "yaml",
  "toml",
];

/** 走 LSP 的语言 */
const LSP_LANGUAGES = ["typescript", "javascript", "go", "python", "rust"];

const APPLY_CODE_ACTION_COMMAND = "agent-ide.apply-code-action";

/**
 * 当前编辑器。全局注册的 provider / command 只能通过它拿到编辑器 ——
 * `key={activeFile}` 让 `<MonacoEditor>` 每切一次 tab 整体重挂载，闭包里捕获的
 * 实例早被 Monaco 释放了，拿它调 `getSelection()` 是对已释放对象操作。
 */
let currentEditor: editor.IStandaloneCodeEditor | null = null;

/** 挂载时传入实例；最后一个 tab 关掉、编辑器被卸载时必须传 `null`。 */
export function setCurrentEditor(instance: editor.IStandaloneCodeEditor | null) {
  currentEditor = instance;
}

export function getCurrentEditor() {
  return currentEditor;
}

export function isAgentBusy() {
  const state = useAgentStore.getState().state;
  return (
    state !== "idle" && state !== "done" && state !== "error" && state !== "waiting_user"
  );
}

function log(entry: {
  level: "info" | "warn" | "error" | "success";
  source: "agent" | "git" | "fs" | "system";
  message: string;
  details?: string;
}) {
  useLogStore.getState().addLog({ time: new Date().toLocaleTimeString(), ...entry });
}

/** 当前选区，连同它属于哪个编辑器；没有选中内容就返回 `null`。 */
function readSelection() {
  const activeEditor = currentEditor;
  if (!activeEditor) return null;
  const selection = activeEditor.getSelection();
  if (!selection || selection.isEmpty()) return null;
  const model = activeEditor.getModel();
  if (!model) return null;
  const text = model.getValueInRange(selection);
  if (!text) return null;
  return { text, startLine: selection.startLineNumber, endLine: selection.endLineNumber };
}

/**
 * 右键菜单和灯泡共用的那一个动作。文件名和文件内容都 `getState()` 现取：注册只
 * 发生一次，而这里是很久之后被点的，闭包捕获会把上下文永远钉在挂载那一刻 ——
 * 编辑器通常在还没打开任何文件时挂载，于是 Agent 收到的上下文文件是空的。
 */
export async function runAgentSelectionAction(action: AgentQuickActionKey) {
  const selected = readSelection();
  if (!selected) return;

  const layout = useLayoutStore.getState();
  layout.setAgentView("task");
  if (!layout.rightVisible) layout.toggleRightPanel();

  // 运行中就不再发第二条。灯泡忙时会直接不出现，右键菜单没法按 Agent 状态隐藏，
  // 所以判断落在这里 —— 否则这条 prompt 先被写进对话记录，再被后端的运行独占
  // 守卫拒掉，用户看到的是一条自己发出去却永远没有回复的消息。面板照样打开：
  // 正在跑的那次运行就在里面，那才是"为什么没反应"的答案。
  if (isAgentBusy()) {
    log({
      level: "warn",
      source: "agent",
      message: "Agent is busy; the selection action was not sent.",
      details: "Wait for the current run to finish, or press Stop.",
    });
    return;
  }

  const editorState = useEditorStore.getState();
  const currentFile = editorState.activeFile;
  const prompt = buildActionPrompt(
    action,
    selected.text,
    currentFile,
    selected.startLine,
    selected.endLine
  );

  const agent = useAgentStore.getState();
  agent.addMessage({
    id: `ctx-${Date.now()}`,
    role: "user",
    content: prompt,
    timestamp: Date.now(),
  });

  await agent.sendPrompt({
    prompt,
    contextFiles: currentFile ? [currentFile] : [],
    activeFile: currentFile ?? undefined,
    activeFileContent: currentFile ? editorState.fileContents[currentFile] : undefined,
    selection: selected.text,
    ideMode: "code",
  });
}

const registeredModules = new WeakSet<Monaco>();

/** 幂等：同一个 monaco 模块只注册一次，调用方不必自己守卫。 */
export function registerGlobalMonacoFeatures(monaco: Monaco) {
  if (registeredModules.has(monaco)) return;
  registeredModules.add(monaco);

  registerLocalCompletion(monaco);
  registerLspProviders(monaco);
  registerAgentLightbulb(monaco);
}

function registerLocalCompletion(monaco: Monaco) {
  for (const language of LOCAL_COMPLETION_LANGUAGES) {
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: [".", "/", "\\", "'", "\"", "@", "<"],
      provideCompletionItems: (model, position) => {
        const word = model.getWordUntilPosition(position);
        const range = {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endColumn: word.endColumn,
        };
        const candidates = buildLocalCompletionCandidates({
          content: model.getValue(),
          language: model.getLanguageId(),
          currentWord: word.word,
          linePrefix: model.getLineContent(position.lineNumber).slice(0, position.column - 1),
          openFilePaths: useEditorStore.getState().openFiles.map((file) => file.path),
        });

        return {
          suggestions: candidates.map((candidate) => ({
            label: candidate.label,
            kind: toMonacoCompletionKind(monaco, candidate.kind),
            insertText: candidate.insertText,
            insertTextRules:
              candidate.kind === "snippet"
                ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
                : undefined,
            detail: candidate.detail,
            sortText: `${999 - candidate.score}-${candidate.label}`,
            range,
          })),
        };
      },
    });
  }
}

function registerLspProviders(monaco: Monaco) {
  for (const language of LSP_LANGUAGES) {
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: [".", "\"", "'", "/", "@", "<"],
      provideCompletionItems: async (model, position) => {
        const word = model.getWordUntilPosition(position);
        const file = model.uri.fsPath || model.uri.path;
        const items = await getLspCompletion(file, position.lineNumber - 1, position.column - 1);
        return {
          suggestions: items.map((item) => ({
            label: item.label,
            kind: toMonacoCompletionItemKind(monaco, item.kind),
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

    monaco.languages.registerHoverProvider(language, {
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

    monaco.languages.registerDefinitionProvider(language, {
      provideDefinition: async (model, position) => {
        const file = model.uri.fsPath || model.uri.path;
        const locations = await getLspDefinition(file, position.lineNumber - 1, position.column - 1);
        return locations.map((location) => ({
          uri: monaco.Uri.file(location.file),
          range: lspRangeToMonacoRange(location.range),
        }));
      },
    });

    monaco.languages.registerDocumentSymbolProvider(language, {
      provideDocumentSymbols: async (model) => {
        const file = model.uri.fsPath || model.uri.path;
        const symbols = await getLspDocumentSymbols(file);
        return flattenDocumentSymbols(monaco, symbols);
      },
    });

    monaco.languages.registerRenameProvider(language, {
      provideRenameEdits: async (model, position, newName) => {
        const file = model.uri.fsPath || model.uri.path;
        const edit = await getLspRename(file, position.lineNumber - 1, position.column - 1, newName);
        if (!edit) return { edits: [] };
        return workspaceEditToMonaco(monaco, edit);
      },
    });

    monaco.languages.registerCodeActionProvider(language, {
      provideCodeActions: async (model, range) => {
        const file = model.uri.fsPath || model.uri.path;
        const diagnostics = markersToLspDiagnostics(
          monaco,
          file,
          monaco.editor
            .getModelMarkers({ resource: model.uri })
            .filter((marker) => markerIntersectsRange(marker, range))
        );
        const actions = await getLspCodeActions(file, monacoRangeToLspRange(range), diagnostics);
        return {
          actions: actions
            .filter((action) => action.edit?.edits.length)
            .map((action) => ({
              title: action.title,
              kind: action.kind || "quickfix",
              command: {
                id: APPLY_CODE_ACTION_COMMAND,
                title: action.title,
                arguments: [action.title, action.edit!],
              },
            })),
          dispose: () => {},
        };
      },
    });
  }

  // 命令 id 是全局唯一的，跟语言无关 —— 放进上面的循环等于同一个 id 注册五遍。
  monaco.editor.registerCommand(
    APPLY_CODE_ACTION_COMMAND,
    async (_accessor, title: string, edit: LspWorkspaceEdit) => {
      await applyLspCodeAction(monaco, title, edit);
    }
  );
}

export async function applyLspCodeAction(
  monaco: Monaco,
  title: string,
  edit: LspWorkspaceEdit
) {
  const failed = (details: string) =>
    log({
      level: "error",
      source: "system",
      message: `Code action failed: ${title}`,
      details,
    });

  const activeEditor = currentEditor;
  if (!activeEditor) {
    failed("No active editor to apply the workspace edit to.");
    return;
  }
  try {
    if (!applyWorkspaceEdit(activeEditor, monaco, edit)) {
      failed("Monaco rejected the workspace edit.");
      return;
    }
    syncWorkspaceEditToStore(monaco, edit);
    await syncWorkspaceEditToLsp(monaco, edit);
    log({
      level: "success",
      source: "system",
      message: `Code action applied: ${title}`,
      details: `${edit.edits.length} edit(s) applied.`,
    });
  } catch (error) {
    failed(String(error));
  }
}

function registerAgentLightbulb(monaco: Monaco) {
  monaco.languages.registerCodeActionProvider("*", {
    provideCodeActions: () => {
      const empty = { actions: [], dispose: () => {} };
      // 忙的时候不给灯泡；光标停着（没选中内容）也不给
      if (isAgentBusy() || !readSelection()) return empty;
      return {
        actions: AGENT_QUICK_ACTIONS.map((act) => ({
          title: `${act.icon} ${act.label} with Agent`,
          kind: "refactor.rewrite",
          diagnostics: [],
          command: {
            id: `agent-ide.lightbulb-${act.key}`,
            title: `${act.label} with Agent`,
          },
        })),
        dispose: () => {},
      };
    },
  });

  for (const act of AGENT_QUICK_ACTIONS) {
    monaco.editor.registerCommand(`agent-ide.lightbulb-${act.key}`, () => {
      void runAgentSelectionAction(act.key as AgentQuickActionKey);
    });
  }
}

function toMonacoCompletionKind(monaco: Monaco, kind: CompletionCandidateKind) {
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

function toMonacoCompletionItemKind(monaco: Monaco, kind?: number) {
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
  monaco: Monaco,
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
  monaco: Monaco,
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
  activeEditor: editor.IStandaloneCodeEditor,
  monaco: Monaco,
  edit: LspWorkspaceEdit
) {
  const activeModel = activeEditor.getModel();
  if (!activeModel) return false;
  const activeFile = activeModel.uri.fsPath || activeModel.uri.path;
  const activeFileEdits = edit.edits.filter((textEdit) => pathsEqual(textEdit.file, activeFile));
  const otherFileEdits = edit.edits.filter((textEdit) => !pathsEqual(textEdit.file, activeFile));

  const activeApplied = activeFileEdits.length
    ? activeEditor.executeEdits(
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

function syncWorkspaceEditToStore(monaco: Monaco, edit: LspWorkspaceEdit) {
  const updateFileContent = useEditorStore.getState().updateFileContent;
  for (const file of new Set(edit.edits.map((textEdit) => textEdit.file))) {
    const model = findModelForFile(monaco, file);
    if (model) updateFileContent(file, model.getValue());
  }
}

async function syncWorkspaceEditToLsp(monaco: Monaco, edit: LspWorkspaceEdit) {
  const touchedFiles = new Set(edit.edits.map((textEdit) => textEdit.file));
  await Promise.all(
    [...touchedFiles].map(async (file) => {
      const model = findModelForFile(monaco, file);
      if (!model || !isLspLanguage(model.getLanguageId())) return;
      await changeLspFile(file, model.getValue(), model.getLanguageId(), model.getVersionId());
    })
  );
}

function findModelForFile(monaco: Monaco, file: string) {
  return (
    monaco.editor.getModel(monaco.Uri.file(file)) ??
    monaco.editor
      .getModels()
      .find((model) => pathsEqual(model.uri.fsPath || model.uri.path, file))
  );
}

function markersToLspDiagnostics(
  monaco: Monaco,
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
  monaco: Monaco,
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
