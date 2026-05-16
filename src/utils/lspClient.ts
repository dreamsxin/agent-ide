import { invoke } from "@tauri-apps/api/core";
import type { IRange } from "monaco-editor";
import { isTauriRuntime } from "./tauri";

export interface LspPosition {
  line: number;
  character: number;
}

export interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

export interface LspDiagnostic {
  file: string;
  range: LspRange;
  severity: "error" | "warning" | "info";
  message: string;
  source?: string;
}

export interface LspHover {
  contents: string;
  range?: LspRange;
}

export interface LspLocation {
  file: string;
  range: LspRange;
}

export interface LspCompletionItem {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: string;
  insertText?: string;
  sortText?: string;
  filterText?: string;
}

export interface LspDocumentSymbol {
  name: string;
  kind: number;
  range: LspRange;
  selectionRange: LspRange;
  children: LspDocumentSymbol[];
}

export interface LspTextEdit {
  file: string;
  range: LspRange;
  newText: string;
}

export interface LspWorkspaceEdit {
  edits: LspTextEdit[];
}

export interface LspCodeAction {
  title: string;
  kind?: string;
  edit?: LspWorkspaceEdit;
}

export interface LspStatusSnapshot {
  status: string;
  message: string;
  workspaceRoot?: string;
  languageId: string;
  languageName: string;
  serverPath?: string;
  serverSource?: string;
  installCommand: string;
  workspaceConfigFiles: string[];
  indexingStatus: string;
  indexingMessage: string;
  openedDocuments: number;
  changeCount: number;
  diagnosticsCount: number;
  lastError?: string;
}

export function isLspLanguage(languageId: string) {
  return ["typescript", "javascript", "go"].includes(languageId);
}

export function toLspLanguageId(languageId: string) {
  if (languageId === "typescript") return "typescript";
  if (languageId === "javascript") return "javascript";
  if (languageId === "go") return "go";
  return languageId;
}

export async function initializeLsp(workspacePath: string | null, languageId = "typescript"): Promise<{ ready: boolean; message?: string }> {
  if (!isTauriRuntime()) return { ready: false, message: "Language server support is available in the Tauri app runtime." };
  try {
    await invoke("lsp_initialize", { workspacePath, languageId });
    return { ready: true };
  } catch (error) {
    console.warn("Language server unavailable:", error);
    return { ready: false, message: String(error) };
  }
}

export async function getLspStatus() {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<LspStatusSnapshot>("lsp_status");
  } catch (error) {
    console.warn("Read TypeScript LSP status failed:", error);
    return null;
  }
}

export async function probeLsp(workspacePath: string | null, languageId = "typescript") {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<LspStatusSnapshot>("lsp_probe", { workspacePath, languageId });
  } catch (error) {
    console.warn("Probe language server failed:", error);
    return null;
  }
}

export async function openLspFile(file: string, content: string, languageId: string, version: number) {
  if (!isTauriRuntime()) return;
  await invoke("lsp_open_file", {
    request: { file, content, languageId: toLspLanguageId(languageId), version },
  });
}

export async function changeLspFile(file: string, content: string, languageId: string, version: number) {
  if (!isTauriRuntime()) return;
  await invoke("lsp_change_file", {
    request: { file, content, languageId: toLspLanguageId(languageId), version },
  });
}

export async function getLspHover(file: string, line: number, character: number) {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<LspHover | null>("lsp_hover", { file, line, character });
  } catch {
    return null;
  }
}

export async function getLspCompletion(file: string, line: number, character: number) {
  if (!isTauriRuntime()) return [];
  try {
    return await invoke<LspCompletionItem[]>("lsp_completion", { file, line, character });
  } catch {
    return [];
  }
}

export async function getLspDefinition(file: string, line: number, character: number) {
  if (!isTauriRuntime()) return [];
  try {
    return await invoke<LspLocation[]>("lsp_definition", { file, line, character });
  } catch {
    return [];
  }
}

export async function getLspDocumentSymbols(file: string) {
  if (!isTauriRuntime()) return [];
  try {
    return await invoke<LspDocumentSymbol[]>("lsp_document_symbols", { file });
  } catch {
    return [];
  }
}

export async function getLspRename(file: string, line: number, character: number, newName: string) {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<LspWorkspaceEdit | null>("lsp_rename", {
      file,
      line,
      character,
      newName,
    });
  } catch {
    return null;
  }
}

export async function getLspCodeActions(
  file: string,
  range: LspRange,
  diagnostics: LspDiagnostic[] = []
) {
  if (!isTauriRuntime()) return [];
  try {
    return await invoke<LspCodeAction[]>("lsp_code_actions", { file, range, diagnostics });
  } catch {
    return [];
  }
}

export function lspRangeToMonacoRange(range: LspRange): IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

export function monacoRangeToLspRange(range: IRange): LspRange {
  return {
    start: {
      line: Math.max(0, range.startLineNumber - 1),
      character: Math.max(0, range.startColumn - 1),
    },
    end: {
      line: Math.max(0, range.endLineNumber - 1),
      character: Math.max(0, range.endColumn - 1),
    },
  };
}

export function toMonacoSymbolKind(monaco: typeof import("monaco-editor"), kind: number) {
  const symbolKind = monaco.languages.SymbolKind;
  const mapping: Record<number, number> = {
    2: symbolKind.Module,
    3: symbolKind.Namespace,
    4: symbolKind.Package,
    5: symbolKind.Class,
    6: symbolKind.Method,
    7: symbolKind.Property,
    8: symbolKind.Field,
    9: symbolKind.Constructor,
    10: symbolKind.Enum,
    11: symbolKind.Interface,
    12: symbolKind.Function,
    13: symbolKind.Variable,
    14: symbolKind.Constant,
    15: symbolKind.String,
    16: symbolKind.Number,
    17: symbolKind.Boolean,
    18: symbolKind.Array,
    22: symbolKind.Struct,
    23: symbolKind.Event,
    24: symbolKind.Operator,
    25: symbolKind.TypeParameter,
  };
  return mapping[kind] ?? symbolKind.Variable;
}
