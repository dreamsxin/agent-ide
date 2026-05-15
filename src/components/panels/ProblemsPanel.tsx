import { useMemo } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore, type ProblemSeverity } from "../../stores/useProblemStore";

const SEVERITY_STYLE: Record<ProblemSeverity, { label: string; color: string }> = {
  error: { label: "E", color: "text-diff-remove" },
  warning: { label: "W", color: "text-diff-modify" },
  info: { label: "I", color: "text-accent-blue" },
};

export default function ProblemsPanel() {
  const problems = useProblemStore((s) => s.problems);
  const clearProblems = useProblemStore((s) => s.clearProblems);
  const openFile = useEditorStore((s) => s.openFile);
  const setActiveFile = useEditorStore((s) => s.setActiveFile);
  const openFiles = useEditorStore((s) => s.openFiles);

  const counts = useMemo(
    () => ({
      error: problems.filter((problem) => problem.severity === "error").length,
      warning: problems.filter((problem) => problem.severity === "warning").length,
      info: problems.filter((problem) => problem.severity === "info").length,
    }),
    [problems]
  );

  const handleProblemClick = async (file: string) => {
    if (!file || file === "Agent") return;
    const existing = openFiles.find((tab) => tab.path === file);
    if (existing) {
      setActiveFile(file);
      return;
    }
    await openFile({
      path: file,
      name: file.split(/[\\/]/).pop() || file,
      language: "",
      isDirty: false,
    });
  };

  return (
    <div className="flex h-full flex-col bg-black text-xs">
      <div className="flex items-center gap-3 border-b border-surface-border px-3 py-1.5">
        <span className="font-semibold text-surface-text">Problems</span>
        <span className="text-diff-remove">{counts.error} errors</span>
        <span className="text-diff-modify">{counts.warning} warnings</span>
        <span className="text-accent-blue">{counts.info} info</span>
        <div className="flex-1" />
        {problems.length > 0 && (
          <button
            onClick={() => clearProblems()}
            className="rounded border border-surface-border px-2 py-0.5 text-[11px] text-surface-muted hover:text-surface-text"
          >
            Clear
          </button>
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
              <button
                key={problem.id}
                onClick={() => void handleProblemClick(problem.file)}
                className="grid w-full grid-cols-[24px_minmax(120px,1fr)_80px_90px] items-start gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left hover:bg-surface-border/20"
              >
                <span className={`font-bold ${style.color}`}>{style.label}</span>
                <span className="min-w-0">
                  <span className="block truncate text-surface-text">{problem.message}</span>
                  <span className="block truncate font-mono text-[10px] text-surface-muted">
                    {problem.file}
                  </span>
                </span>
                <span className="font-mono text-[10px] text-surface-muted">
                  {problem.line}:{problem.column}
                </span>
                <span className="truncate text-[10px] uppercase text-surface-muted">
                  {problem.source}
                </span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
