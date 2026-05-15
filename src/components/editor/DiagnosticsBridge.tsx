import { useEffect } from "react";
import { useProblemStore, type ProblemEntry, type ProblemSeverity } from "../../stores/useProblemStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { normalizeFilePath, pathKey } from "../../utils/paths";
import { useMonacoContext } from "./MonacoContext";

export default function DiagnosticsBridge() {
  const { monaco } = useMonacoContext();
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const replaceProblems = useProblemStore((s) => s.replaceProblems);

  useEffect(() => {
    if (!monaco) return;

    const syncMarkers = () => {
      const knownPaths = new Map(openFiles.map((file) => [pathKey(file.path), file.path]));
      const markerProblems = monaco.editor
        .getModelMarkers({})
        .filter((marker) => marker.owner !== "lsp")
        .filter((marker) => marker.owner !== "runtime-problems")
        .filter((marker) => knownPaths.has(pathKey(markerResourcePath(marker))))
        .map((marker): ProblemEntry => {
          const file = knownPaths.get(pathKey(markerResourcePath(marker))) ?? normalizeFilePath(markerResourcePath(marker));
          return {
            id: `diagnostic-${pathKey(file)}-${marker.startLineNumber}-${marker.startColumn}-${marker.code ?? marker.message}`,
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

function markerResourcePath(marker: import("monaco-editor").editor.IMarker) {
  return marker.resource.fsPath || marker.resource.path;
}
