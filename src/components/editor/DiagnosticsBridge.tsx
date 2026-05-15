import { useEffect } from "react";
import { useProblemStore, type ProblemEntry, type ProblemSeverity } from "../../stores/useProblemStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useMonacoContext } from "./MonacoContext";

export default function DiagnosticsBridge() {
  const { monaco } = useMonacoContext();
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const replaceProblems = useProblemStore((s) => s.replaceProblems);

  useEffect(() => {
    if (!monaco) return;

    const syncMarkers = () => {
      const knownPaths = new Set(openFiles.map((file) => normalizePath(file.path)));
      const markerProblems = monaco.editor
        .getModelMarkers({})
        .filter((marker) => knownPaths.has(normalizePath(marker.resource.fsPath || marker.resource.path)))
        .map((marker): ProblemEntry => {
          const file = marker.resource.fsPath || marker.resource.path;
          return {
            id: `diagnostic-${normalizePath(file)}-${marker.startLineNumber}-${marker.startColumn}-${marker.code ?? marker.message}`,
            file,
            line: marker.startLineNumber,
            column: marker.startColumn,
            severity: toProblemSeverity(monaco, marker.severity),
            source: "diagnostic",
            message: marker.message,
          };
        });

      replaceProblems("diagnostic", markerProblems);
    };

    syncMarkers();
    const disposable = monaco.editor.onDidChangeMarkers(syncMarkers);
    return () => disposable.dispose();
  }, [activeFile, monaco, openFiles, replaceProblems]);

  return null;
}

function toProblemSeverity(
  monaco: typeof import("monaco-editor"),
  severity: number
): ProblemSeverity {
  if (severity === monaco.MarkerSeverity.Error) return "error";
  if (severity === monaco.MarkerSeverity.Warning) return "warning";
  return "info";
}

function normalizePath(path: string) {
  return path.replace(/\\/g, "/").toLowerCase();
}
