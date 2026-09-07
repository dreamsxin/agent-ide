import { describe, expect, it } from "vitest";
import { buildTaskFailureFixPrompt } from "./agentRuntimeContext";
import type { ProjectTaskRunState } from "../stores/useTaskStore";

function failedRun(overrides: Partial<ProjectTaskRunState> = {}): ProjectTaskRunState {
  return {
    taskId: "lint",
    label: "Lint",
    command: "npm run lint",
    status: "failed",
    exitCode: 1,
    output: "src/app.ts(3,7): error TS2322: Type 'number' is not assignable to type 'string'.",
    startedAt: 1,
    finishedAt: 2,
    durationMs: 1,
    runId: "run-1",
    ...overrides,
  } as ProjectTaskRunState;
}

describe("buildTaskFailureFixPrompt", () => {
  // 之前提示里只有命令名和退出码，输出由 buildIdeRuntimeContext 单独附加，
  // 而那边取的是"最近一次失败的任务" —— 点旧任务的 Fix 会配上别的命令的输出。
  it("carries the clicked run's own output", () => {
    const prompt = buildTaskFailureFixPrompt(failedRun());

    expect(prompt).toContain("npm run lint");
    expect(prompt).toContain("Exit code: 1");
    expect(prompt).toContain("TS2322");
  });

  it("says so explicitly when no output was captured", () => {
    const prompt = buildTaskFailureFixPrompt(failedRun({ output: "   " }));

    // 留空会让模型以为命令静默通过了
    expect(prompt).toContain("(no output captured)");
  });

  it("keeps only the tail of very long output", () => {
    const prompt = buildTaskFailureFixPrompt(
      failedRun({ output: `${"noise\n".repeat(2000)}FINAL FAILURE LINE` })
    );

    // 报错通常在末尾，截断要保尾巴而不是保开头
    expect(prompt).toContain("FINAL FAILURE LINE");
    expect(prompt.length).toBeLessThan(4500);
  });
});
