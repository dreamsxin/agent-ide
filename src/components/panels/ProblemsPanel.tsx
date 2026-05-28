import { useMemo } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore, type ProblemSeverity } from "../../stores/useProblemStore";
import { useFixWithAgent } from "../../hooks/useFixWithAgent";
import { fileNameFromPath, normalizeFilePath, pathsEqual } from "../../utils/paths";

const SEVERITY_STYLE: Record<ProblemSeverity, { label: string; color: string }> = {
  error: { label: "E", color: "text-diff-remove" },
  warning: { label: "W", color: "text-diff-modify" },
  info: { label: "I", color: "text-accent-blue" },
};

export default function ProblemsPanel() {
  const problems = useProblemStore((s) => s.problems);
  const clearProblems = useProblemStore((s) => s.clearProblems);
  const removeProblem = useProblemStore((s) => s.removeProblem);
  const openFile = useEditorStore((s) => s.openFile);
  const setActiveFile = useEditorStore((s) => s.setActiveFile);
  const revealLocation = useEditorStore((s) => s.revealLocation);
  const openFiles = useEditorStore((s) => s.openFiles);
  const { explainProblem, fixProblem, isAgentBusy } = useFixWithAgent();

  const counts = useMemo(
    () => ({
      error: problems.filter((problem) => problem.severity === "error").length,
      warning: problems.filter((problem) => problem.severity === "warning").length,
      info: problems.filter((problem) => problem.severity === "info").length,
    }),
    [problems]
  );

  const handleProblemClick = async (file: string, line: number, column: number) => {
    if (!file || file === "Agent") return;
    const normalizedFile = normalizeFilePath(file);
    const existing = openFiles.find((tab) => pathsEqual(tab.path, normalizedFile));
    if (existing) {
      setActiveFile(existing.path);
      revealLocation(existing.path, line, column);
      return;
    }
    await openFile({
      path: normalizedFile,
      name: fileNameFromPath(normalizedFile),
      language: "",
      isDirty: false,
    });
    revealLocation(normalizedFile, line, column);
  };

  return (
    <div data-testid="problems-panel" className="flex h-full flex-col bg-black text-xs">
      <div className="flex items-center gap-3 border-b border-surface-border px-3 py-1.5">
        <span className="font-semibold text-surface-text">Problems</span>
        <span className="text-diff-remove">{counts.error} errors</span>
        <span className="text-diff-modify">{counts.warning} warnings</span>
        <span className="text-accent-blue">{counts.info} info</span>
        <div className="flex-1" />
        {problems.length > 0 && (
          <>
            <button
              onClick={() => void fixProblem()}
              disabled={isAgentBusy}
              data-testid="problems-fix-with-agent"
              className="rounded border border-accent-blue/40 px-2 py-0.5 text-[11px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
            >
              Fix with Agent
            </button>
            <button
              onClick={() => clearProblems()}
              className="rounded border border-surface-border px-2 py-0.5 text-[11px] text-surface-muted hover:text-surface-text"
            >
              Clear
            </button>
          </>
        )}
      </div>

      <div className="flex-1 overflow-auto">
        {problems.length === 0 ? (
          <div className="flex h-full items-center justify-center text-surface-muted">
            No problems reported.
          </div>
        ) : (
          problems.map((problem) => {
            const style = SEVERITY_STYLE[problem.severity];
            return (
              <div
                key={problem.id}
                onClick={() => void handleProblemClick(problem.file, problem.line, problem.column)}
                data-testid="problem-row"
                className="grid w-full grid-cols-[24px_minmax(120px,1fr)_80px_max-content] items-start gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left hover:bg-surface-border/20"
              >
                <span className={`font-bold ${style.color}`}>{style.label}</span>
                <span className="min-w-0">
                  <span className="block truncate text-surface-text">{problem.message}</span>
                  <span className="block truncate font-mono text-[10px] text-surface-muted">
                    {formatProblemLocation(problem.file, problem.line, problem.column)}
                  </span>
                </span>
                <span className="truncate text-[10px] uppercase text-surface-muted">
                  {problem.source} &middot; {problem.severity}
                </span>
                <span className="flex items-center gap-1">
                  <button
                    onClick={(event) => {
                      event.stopPropagation();
                      void explainProblem(problem);
                    }}
                    disabled={isAgentBusy}
                    className="rounded border border-accent-green/30 px-1.5 py-0.5 text-[10px] text-accent-green hover:bg-accent-green/10 disabled:cursor-not-allowed disabled:opacity-40"
                    title="Ask Agent to explain this problem"
                  >
                    Explain
                  </button>
                  <button
                    onClick={(event) => {
                      event.stopPropagation();
                      void fixProblem(problem);
                    }}
                    disabled={isAgentBusy}
                    data-testid="problem-fix-with-agent"
                    className="rounded border border-accent-blue/30 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    Fix
                  </button>
                  <button
                    onClick={(event) => {
                      event.stopPropagation();
                      removeProblem(problem.id);
                    }}
                    className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text hover:border-surface-muted"
                    title="Ignore this problem"
                  >
                    Ignore
                  </button>
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function formatProblemLocation(file: string, line: number, column: number) {
  return `${file} (${line}:${column})`;
}
