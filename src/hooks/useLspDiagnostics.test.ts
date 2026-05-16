import { describe, expect, it } from "vitest";
import { summarizeLspDiagnostics } from "./useLspDiagnostics";

describe("summarizeLspDiagnostics", () => {
  it("counts diagnostics by severity and normalizes the file path", () => {
    const summary = summarizeLspDiagnostics({
      file: "file:///D:/work/demo/src/app.ts",
      diagnostics: [
        diagnostic("error"),
        diagnostic("warning"),
        diagnostic("warning"),
        diagnostic("info"),
      ],
    });

    expect(summary).toEqual({
      file: "D:/work/demo/src/app.ts",
      error: 1,
      warning: 2,
      info: 1,
    });
  });
});

function diagnostic(severity: "error" | "warning" | "info") {
  return {
    file: "D:/work/demo/src/app.ts",
    range: {
      start: { line: 0, character: 0 },
      end: { line: 0, character: 1 },
    },
    severity,
    message: severity,
    source: "typescript",
  };
}
