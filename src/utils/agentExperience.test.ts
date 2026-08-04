import { describe, expect, it } from "vitest";
import type { DiffEntry, Step } from "../types/agent";
import { agentStateLabel, isAgentBusy, summarizeAgentRun } from "./agentExperience";

const step = (id: string, status: Step["status"]): Step => ({
  id,
  title: id,
  type: "edit",
  status,
  logs: [],
});

const diff = (id: string, status: DiffEntry["status"]): DiffEntry => ({
  id,
  file: `${id}.ts`,
  hunks: [],
  status,
});

describe("agent experience helpers", () => {
  it("summarizes progress and reviewable changes", () => {
    const summary = summarizeAgentRun(
      [step("one", "done"), step("two", "doing"), step("three", "skipped")],
      [diff("one", "pending"), diff("two", "applied"), diff("three", "failed")]
    );

    expect(summary.completedSteps).toBe(2);
    expect(summary.totalSteps).toBe(3);
    expect(summary.progressPercent).toBe(67);
    expect(summary.pendingChanges).toBe(2);
    expect(summary.activeStep?.id).toBe("two");
  });

  it("keeps empty runs at zero progress", () => {
    expect(summarizeAgentRun([], [])).toEqual({
      activeStep: null,
      completedSteps: 0,
      pendingChanges: 0,
      progressPercent: 0,
      totalSteps: 0,
    });
  });

  it("uses task-oriented state labels", () => {
    expect(agentStateLabel("waiting_user")).toBe("Needs review");
    expect(agentStateLabel("planning")).toBe("Building plan");
    expect(isAgentBusy("acting")).toBe(true);
    expect(isAgentBusy("done")).toBe(false);
  });
});
