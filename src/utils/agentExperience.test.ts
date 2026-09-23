import { describe, expect, it } from "vitest";
import type { DiffEntry, Step } from "../types/agent";
import { agentStateMessageKey, isAgentBusy, summarizeAgentRun } from "./agentExperience";
import { translate } from "../i18n";

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
    expect(summary.nextStep?.id).toBe("two");
    expect(summary.reviewRequired).toBe(true);
  });

  it("keeps empty runs at zero progress", () => {
    expect(summarizeAgentRun([], [])).toEqual({
      activeStep: null,
      completedSteps: 0,
      nextStep: null,
      pendingChanges: 0,
      progressPercent: 0,
      reviewRequired: false,
      totalSteps: 0,
    });
  });

  it("prioritizes failed work before the next todo step", () => {
    const summary = summarizeAgentRun(
      [step("done", "done"), step("later", "todo"), step("failed", "error")],
      []
    );

    expect(summary.nextStep?.id).toBe("failed");
  });

  /** 状态名只留一份映射：这里给键，句子由 `t()` 按语言给 */
  it("maps every state to a message key", () => {
    expect(agentStateMessageKey("waiting_user")).toBe("state.waiting_user");
    expect(agentStateMessageKey("planning")).toBe("state.planning");
    expect(translate("zh", agentStateMessageKey("waiting_user"))).toBe("等你回答");
    expect(translate("en", agentStateMessageKey("planning"))).toBe("Planning");
    expect(isAgentBusy("acting")).toBe(true);
    expect(isAgentBusy("done")).toBe(false);
  });

  /**
   * `reverted` 是终态：工具写入的记录被撤销之后停在这里。它既不是待办也不该让状态栏
   * 一直说"需要审查" —— 用户刚刚亲手撤销了它。
   */
  it("撤销掉的工具写入记录不算待处理的改动", () => {
    const summary = summarizeAgentRun([], [diff("undone", "reverted")]);

    expect(summary.pendingChanges).toBe(0);
    expect(summary.reviewRequired).toBe(false);
  });
});
