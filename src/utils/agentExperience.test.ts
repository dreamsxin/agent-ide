import { describe, expect, it } from "vitest";
import type { DiffEntry, Step } from "../types/agent";
import { agentRunIsLive, agentStateMessageKey, isAgentBusy, summarizeAgentRun } from "./agentExperience";
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

/**
 * 「忙」和「还有一次运行没结束」是两件事，差的正是 `waiting_user`。
 *
 * 顶栏曾经只有一份自己手写的判断，把 `waiting_user` 算成了忙：恢复一次被中断的会话
 * 就落在这个状态上（见 `normalizeRestoredAgentState`），于是「写代码 / 先做计划」两个
 * 按钮永久禁用 —— 模式再也换不回来，而 Agent 根本没在跑。
 */
describe("busy 和 live 的分界", () => {
  it("等用户回答的时候不算忙", () => {
    expect(isAgentBusy("waiting_user")).toBe(false);
    expect(isAgentBusy("idle")).toBe(false);
    expect(isAgentBusy("done")).toBe(false);
    expect(isAgentBusy("error")).toBe(false);
    for (const state of ["thinking", "planning", "acting", "reviewing"] as const) {
      expect(isAgentBusy(state)).toBe(true);
    }
  });

  it("但那次运行还挂着，所以 Stop 仍然有意义", () => {
    expect(agentRunIsLive("waiting_user")).toBe(true);
    expect(agentRunIsLive("acting")).toBe(true);
    expect(agentRunIsLive("idle")).toBe(false);
    expect(agentRunIsLive("done")).toBe(false);
    expect(agentRunIsLive("error")).toBe(false);
  });
});
