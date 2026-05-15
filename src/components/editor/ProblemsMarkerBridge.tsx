import { useEffect, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore, type ProblemEntry } from "../../stores/useProblemStore";
import { useMonacoContext } from "./MonacoContext";
import { pathKey } from "../../utils/paths";

const RUNTIME_MARKER_OWNER = "runtime-problems";

export default function ProblemsMarkerBridge() {
  const { editor, monaco } = useMonacoContext();
  const problems = useProblemStore((s) => s.problems);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const decorationIdsRef = useRef<string[]>([]);

  useEffect(() => {
    if (!monaco) return;

    const runtimeProblems = problems.filter((problem) =>
      ["test", "agent", "system"].includes(problem.source)
    );
    const runtimeProblemsByFile = groupByFile(runtimeProblems);
    const allProblemsByFile = groupByFile(problems);
    const touchedModels = new Set<string>();

    const models = monaco.editor.getModels();
    const activeModel = editor?.getModel() ?? null;

    for (const model of models) {
      const key = pathKey(model.uri.fsPath || model.uri.path);
      const fileProblems =
        runtimeProblemsByFile.get(key) ??
        (activeModel === model && activeFile ? runtimeProblemsByFile.get(pathKey(activeFile)) : undefined);
      if (!fileProblems?.length) continue;

      touchedModels.add(key);
      monaco.editor.setModelMarkers(
        model,
        RUNTIME_MARKER_OWNER,
        fileProblems.map((problem) => toMarker(monaco, model, problem))
      );
    }

    for (const model of models) {
      const key = pathKey(model.uri.fsPath || model.uri.path);
      if (!touchedModels.has(key)) {
        monaco.editor.setModelMarkers(model, RUNTIME_MARKER_OWNER, []);
      }
    }

    if (editor) {
      const activeModel = editor.getModel();
      const activeKey = activeModel ? pathKey(activeModel.uri.fsPath || activeModel.uri.path) : null;
      const activeProblems =
        activeKey ? allProblemsByFile.get(activeKey) ?? (activeFile ? allProblemsByFile.get(pathKey(activeFile)) : undefined) : undefined;
      decorationIdsRef.current = editor.deltaDecorations(
        decorationIdsRef.current,
        activeModel && activeProblems?.length
          ? activeProblems.map((problem) => toDecoration(monaco, activeModel, problem))
          : []
      );
    }
  }, [activeFile, editor, monaco, openFiles, problems]);

  useEffect(() => {
    return () => {
      if (editor) {
        editor.deltaDecorations(decorationIdsRef.current, []);
      }
    };
  }, [editor]);

  return null;
}

function toDecoration(
  monaco: typeof import("monaco-editor"),
  model: import("monaco-editor").editor.ITextModel,
  problem: ProblemEntry
) {
  const line = clamp(problem.line, 1, model.getLineCount());
  return {
    range: new monaco.Range(line, 1, line, model.getLineMaxColumn(line)),
    options: {
      isWholeLine: true,
      className: `runtime-problem-line runtime-problem-line-${problem.severity}`,
      glyphMarginClassName: `runtime-problem-glyph runtime-problem-glyph-${problem.severity}`,
      linesDecorationsClassName: `runtime-problem-line-number runtime-problem-line-number-${problem.severity}`,
      overviewRuler: {
        color: decorationColor(problem.severity),
        position: monaco.editor.OverviewRulerLane.Right,
      },
      minimap: {
        color: decorationColor(problem.severity),
        position: monaco.editor.MinimapPosition.Inline,
      },
      hoverMessage: { value: problem.message },
    },
  };
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

function toMarker(
  monaco: typeof import("monaco-editor"),
  model: import("monaco-editor").editor.ITextModel,
  problem: ProblemEntry
) {
  const line = clamp(problem.line, 1, model.getLineCount());
  const maxColumn = model.getLineMaxColumn(line);
  const startColumn = clamp(problem.column, 1, Math.max(1, maxColumn - 1));
  return {
    startLineNumber: line,
    startColumn: 1,
    endLineNumber: line,
    endColumn: Math.max(maxColumn, startColumn + 1),
    severity: toMarkerSeverity(monaco, problem.severity),
    message: problem.message,
    source: problem.source,
  };
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.min(max, value));
}

function decorationColor(severity: ProblemEntry["severity"]) {
  if (severity === "error") return "#DA3633";
  if (severity === "warning") return "#D29922";
  return "#3B82F6";
}
