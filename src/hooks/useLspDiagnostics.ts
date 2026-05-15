import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useProblemStore, type ProblemEntry } from "../stores/useProblemStore";
import { useLspStore, type LspStatus } from "../stores/useLspStore";
import { lspRangeToMonacoRange, type LspDiagnostic } from "../utils/lspClient";
import { normalizeFilePath, pathKey } from "../utils/paths";
import { isTauriRuntime } from "../utils/tauri";

interface LspDiagnosticsEvent {
  file: string;
  diagnostics: LspDiagnostic[];
}

export function useLspDiagnostics(monaco: typeof import("monaco-editor") | null) {
  const replaceProblems = useProblemStore((s) => s.replaceProblems);
  const setLspStatus = useLspStore((s) => s.setStatus);
  const setDiagnosticSummary = useLspStore((s) => s.setDiagnosticSummary);
  const diagnosticsByFileRef = useRef<Map<string, LspDiagnosticsEvent>>(new Map());

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const unlisten = listen<LspDiagnosticsEvent>("lsp-diagnostics", (event) => {
      diagnosticsByFileRef.current.set(
        pathKey(event.payload.file),
        event.payload
      );

      const problems: ProblemEntry[] = [...diagnosticsByFileRef.current.values()].flatMap((entry) =>
        entry.diagnostics.map((diagnostic) => ({
          id: `lsp-${pathKey(diagnostic.file)}-${diagnostic.range.start.line}-${diagnostic.range.start.character}-${diagnostic.message}`,
          file: normalizeFilePath(diagnostic.file),
          line: diagnostic.range.start.line + 1,
          column: diagnostic.range.start.character + 1,
          severity: diagnostic.severity,
          source: "lsp",
          message: diagnostic.source
            ? `[${diagnostic.source}] ${diagnostic.message}`
            : diagnostic.message,
        }))
      );
      replaceProblems("lsp", problems);
      setDiagnosticSummary(summarizeDiagnostics(event.payload));

      if (monaco) applyMarkers(monaco, event.payload.file, event.payload.diagnostics);
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [monaco, replaceProblems, setDiagnosticSummary]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const unlisten = listen<{ status: LspStatus; message: string }>("lsp-status", (event) => {
      setLspStatus(event.payload.status, event.payload.message);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [setLspStatus]);

  useEffect(() => {
    if (!monaco) return;

    const syncCachedMarkers = () => {
      for (const diagnostics of diagnosticsByFileRef.current.values()) {
        applyMarkers(monaco, diagnostics.file, diagnostics.diagnostics);
      }
    };

    syncCachedMarkers();
    const disposable = monaco.editor.onDidCreateModel(syncCachedMarkers);
    return () => disposable.dispose();
  }, [monaco]);
}

function summarizeDiagnostics(event: LspDiagnosticsEvent) {
  return event.diagnostics.reduce(
    (summary, diagnostic) => {
      summary[diagnostic.severity] += 1;
      return summary;
    },
    {
      file: normalizeFilePath(event.file),
      error: 0,
      warning: 0,
      info: 0,
    }
  );
}

function toMonacoMarkerSeverity(
  monaco: typeof import("monaco-editor"),
  severity: LspDiagnostic["severity"]
) {
  if (severity === "error") return monaco.MarkerSeverity.Error;
  if (severity === "warning") return monaco.MarkerSeverity.Warning;
  return monaco.MarkerSeverity.Info;
}

function applyMarkers(
  monaco: typeof import("monaco-editor"),
  file: string | undefined,
  diagnostics: LspDiagnostic[]
) {
  if (!file) return;
  const model = findModelForFile(monaco, normalizeFilePath(file));
  if (!model) return;

  monaco.editor.setModelMarkers(
    model,
    "lsp",
    diagnostics.map((diagnostic) => ({
      ...lspRangeToMonacoRange(diagnostic.range),
      severity: toMonacoMarkerSeverity(monaco, diagnostic.severity),
      message: diagnostic.message,
      source: diagnostic.source || "typescript-language-server",
    }))
  );
}

function findModelForFile(monaco: typeof import("monaco-editor"), file: string) {
  const target = pathKey(file);
  return (
    monaco.editor.getModel(monaco.Uri.file(file)) ??
    monaco.editor
      .getModels()
      .find((model) => pathKey(model.uri.fsPath || model.uri.path) === target)
  );
}
