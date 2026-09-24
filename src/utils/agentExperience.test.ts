import { describe, expect, it } from "vitest";
import type { DiffEntry, Step } from "../types/agent";
import {
  agentRunIsLive,
  agentStateMessageKey,
  isAgentBusy,
  runDetailMessage,
  summarizeAgentRun,
} from "./agentExperience";
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
    // 中文这句刻意是中性的：`waiting_user` 同时表示"问你一道题"和"改动等你审"，
    // 英文 `Waiting for you` 本来就不指定是哪一种，中文以前写成「等你回答」说多了。
    expect(translate("zh", agentStateMessageKey("waiting_user"))).toBe("等你处理");
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

/**
 * 状态行的第二句不许无根据地说"在对话里等你回答"。
 *
 * 用户报告：改动已经应用完了，面板还写着这句话。原因是这句只看状态是不是
 * `waiting_user`，而那个状态同时表示"跑完了、改动等你审"和"被中断的会话恢复出来"。
 */
describe("状态行第二句", () => {
  const idleSummary = summarizeAgentRun([], []);

  it("真的挂着一道题才说在对话里等你回答", () => {
    expect(
      runDetailMessage({
        summary: idleSummary,
        hasPendingQuestion: true,
        ideModeLabel: "写代码",
        modeLabel: "自动落盘",
      })
    ).toEqual({ key: "summary.detail.waiting" });
  });

  it("没有问题挂着就落回模式那一行，而不是编一个不存在的提问", () => {
    expect(
      runDetailMessage({
        summary: idleSummary,
        hasPendingQuestion: false,
        ideModeLabel: "写代码",
        modeLabel: "自动落盘",
      })
    ).toEqual({
      key: "summary.detail.mode",
      params: { ide: "写代码", mode: "自动落盘" },
    });
  });

  it("有待审改动时先说改动，那是用户下一步真正要做的事", () => {
    expect(
      runDetailMessage({
        summary: summarizeAgentRun([], [diff("one", "pending"), diff("two", "failed")]),
        hasPendingQuestion: true,
        ideModeLabel: "写代码",
        modeLabel: "自动落盘",
      })
    ).toEqual({ key: "summary.detail.review.many", params: { count: 2 } });
  });
});
