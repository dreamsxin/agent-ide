import { create } from "zustand";

export type ProblemSeverity = "error" | "warning" | "info";
export type ProblemSource = "diagnostic" | "lsp" | "test" | "agent" | "system";

export interface ProblemEntry {
  id: string;
  file: string;
  line: number;
  column: number;
  severity: ProblemSeverity;
  source: ProblemSource;
  message: string;
}

interface ProblemStore {
  problems: ProblemEntry[];
  setProblems: (problems: ProblemEntry[]) => void;
  upsertProblems: (source: ProblemSource, problems: ProblemEntry[]) => void;
  replaceProblems: (source: ProblemSource, problems: ProblemEntry[]) => void;
  removeProblem: (id: string) => void;
  clearProblems: (source?: ProblemSource) => void;
}

export const useProblemStore = create<ProblemStore>((set) => ({
  problems: [],

  setProblems: (problems) => set({ problems: sortProblems(problems) }),

  upsertProblems: (source, problems) =>
    set((state) => ({
      problems: sortProblems(upsertById(state.problems, problems.map((problem) => ({ ...problem, source })))),
    })),

  replaceProblems: (source, problems) =>
    set((state) => ({
      problems: sortProblems([
        ...state.problems.filter((problem) => problem.source !== source),
        ...problems.map((problem) => ({ ...problem, source })),
      ]),
    })),

  removeProblem: (id) =>
    set((state) => ({
      problems: state.problems.filter((problem) => problem.id !== id),
    })),

  clearProblems: (source) =>
    set((state) => ({
      problems: source
        ? state.problems.filter((problem) => problem.source !== source)
        : [],
    })),
}));

function upsertById(current: ProblemEntry[], incoming: ProblemEntry[]) {
  const byId = new Map(current.map((problem) => [problem.id, problem]));
  for (const problem of incoming) {
    byId.set(problem.id, problem);
  }
  return [...byId.values()];
}

function sortProblems(problems: ProblemEntry[]) {
  const severityRank: Record<ProblemSeverity, number> = {
    error: 0,
    warning: 1,
    info: 2,
  };

  return [...problems].sort((a, b) => {
    return (
      severityRank[a.severity] - severityRank[b.severity] ||
      a.file.localeCompare(b.file) ||
      a.line - b.line ||
      a.column - b.column
    );
  });
}
