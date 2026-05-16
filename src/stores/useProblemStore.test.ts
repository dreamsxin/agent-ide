import { beforeEach, describe, expect, it } from "vitest";
import { useProblemStore, type ProblemEntry } from "./useProblemStore";

describe("useProblemStore", () => {
  beforeEach(() => {
    useProblemStore.getState().clearProblems();
  });

  it("sorts problems by severity, file, line, and column", () => {
    useProblemStore.getState().setProblems([
      problem("info-a", "info", "b.ts", 3, 1),
      problem("warning-a", "warning", "a.ts", 5, 1),
      problem("error-b", "error", "a.ts", 2, 3),
      problem("error-a", "error", "a.ts", 2, 1),
    ]);

    expect(useProblemStore.getState().problems.map((item) => item.id)).toEqual([
      "error-a",
      "error-b",
      "warning-a",
      "info-a",
    ]);
  });

  it("replaces only the requested source", () => {
    useProblemStore.getState().setProblems([
      problem("lsp-old", "error", "a.ts", 1, 1, "lsp"),
      problem("test-old", "error", "b.ts", 1, 1, "test"),
    ]);

    useProblemStore.getState().replaceProblems("lsp", [
      problem("lsp-new", "warning", "c.ts", 1, 1, "lsp"),
    ]);

    expect(useProblemStore.getState().problems.map((item) => item.id)).toEqual([
      "test-old",
      "lsp-new",
    ]);
  });

  it("upserts incoming problems by id and forces the requested source", () => {
    useProblemStore.getState().setProblems([
      problem("same", "warning", "a.ts", 1, 1, "diagnostic"),
    ]);

    useProblemStore.getState().upsertProblems("agent", [
      problem("same", "error", "a.ts", 2, 1, "lsp"),
    ]);

    expect(useProblemStore.getState().problems).toMatchObject([
      {
        id: "same",
        source: "agent",
        severity: "error",
        line: 2,
      },
    ]);
  });
});

function problem(
  id: string,
  severity: ProblemEntry["severity"],
  file: string,
  line: number,
  column: number,
  source: ProblemEntry["source"] = "diagnostic"
): ProblemEntry {
  return {
    id,
    file,
    line,
    column,
    severity,
    source,
    message: id,
  };
}
