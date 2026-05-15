import { useEffect } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore, type ProblemEntry } from "../../stores/useProblemStore";
import { useMonacoContext } from "./MonacoContext";
import { pathKey } from "../../utils/paths";

const RUNTIME_MARKER_OWNER = "runtime-problems";

export default function ProblemsMarkerBridge() {
  const { monaco } = useMonacoContext();
  const problems = useProblemStore((s) => s.problems);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);

  useEffect(() => {
    if (!monaco) return;

    const runtimeProblems = problems.filter((problem) =>
      ["test", "agent", "system"].includes(problem.source)
    );
    const problemsByFile = groupByFile(runtimeProblems);
    const touchedModels = new Set<string>();

    for (const model of monaco.editor.getModels()) {
      const key = pathKey(model.uri.fsPath || model.uri.path);
      const fileProblems = problemsByFile.get(key);
      if (!fileProblems?.length) continue;

      touchedModels.add(key);
      monaco.editor.setModelMarkers(
        model,
        RUNTIME_MARKER_OWNER,
        fileProblems.map((problem) => ({
          startLineNumber: Math.max(1, problem.line),
          startColumn: Math.max(1, problem.column),
          endLineNumber: Math.max(1, problem.line),
          endColumn: Math.max(2, problem.column + 1),
          severity: toMarkerSeverity(monaco, problem.severity),
          message: problem.message,
          source: problem.source,
        }))
      );
    }

    for (const model of monaco.editor.getModels()) {
      const key = pathKey(model.uri.fsPath || model.uri.path);
      if (!touchedModels.has(key)) {
        monaco.editor.setModelMarkers(model, RUNTIME_MARKER_OWNER, []);
      }
    }
  }, [activeFile, monaco, openFiles, problems]);

  return null;
}

function groupByFile(problems: ProblemEntry[]) {
  const byFile = new Map<string, ProblemEntry[]>();
  for (const problem of problems) {
    if (!problem.file || problem.file === "Agent") continue;
    const key = pathKey(problem.file);
    const current = byFile.get(key) ?? [];
    current.push(problem);
    byFile.set(key, current);
  }
  return byFile;
}

function toMarkerSeverity(
  monaco: typeof import("monaco-editor"),
  severity: ProblemEntry["severity"]
) {
  if (severity === "error") return monaco.MarkerSeverity.Error;
  if (severity === "warning") return monaco.MarkerSeverity.Warning;
  return monaco.MarkerSeverity.Info;
}
