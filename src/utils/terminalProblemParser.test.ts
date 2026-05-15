import { describe, expect, it } from "vitest";
import { appendAndParseTerminalProblems, parseTerminalProblems } from "./terminalProblemParser";

describe("terminalProblemParser", () => {
  it("parses Node ESM file URI stack traces", () => {
    const output = [
      "ReferenceError: xxxxx is not defined",
      "    at file:///D:/work/openclaw-workspace/chrome-mcp-client/test.js:18:1",
      "    at ModuleJob.run (node:internal/modules/esm/module_job:437:25)",
    ].join("\n");

    const problems = parseTerminalProblems(output, "test");

    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({
      file: "D:/work/openclaw-workspace/chrome-mcp-client/test.js",
      line: 18,
      column: 1,
      severity: "error",
      source: "test",
    });
  });

  it("keeps enough terminal buffer to parse split output", () => {
    const first = appendAndParseTerminalProblems(
      "",
      "ReferenceError: xxxxx is not defined\n    at file:///D:/repo/test.js",
      "main"
    );
    const second = appendAndParseTerminalProblems(first.buffer, ":9:3\n", "main");

    expect(second.problems).toHaveLength(1);
    expect(second.problems[0].file).toBe("D:/repo/test.js");
    expect(second.problems[0].line).toBe(9);
    expect(second.problems[0].column).toBe(3);
  });
});
