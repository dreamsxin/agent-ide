import { describe, expect, it } from "vitest";
import { backendStatus, repairStatus, verificationStatus } from "./taskVerification";

const check = (exitCode: number) => ({ command: "npm test", exitCode });

describe("检查和修复的状态行", () => {
  /**
   * 断言落在键上，因为函数决定的就是键：句子里数字放哪、"跳过"放哪是翻译的事。
   * 以前这里拼字符串，于是这几句只有英文一种语序。
   */
  it("全过了就说全过了", () => {
    expect(
      verificationStatus({ failed: 0, skipped: [], repairPrompt: null, results: [check(0)] })
    ).toEqual({ kind: "message", key: "tasks.verify.passed", params: { count: 1 } });
  });

  it("跳过的那几条要单独说，不能混进通过数里", () => {
    expect(
      verificationStatus({
        failed: 0,
        skipped: ["npm run dev"],
        repairPrompt: null,
        results: [check(0), check(0)],
      })
    ).toEqual({
      kind: "message",
      key: "tasks.verify.passed.skipped",
      params: { count: 2, skipped: 1 },
    });
  });

  it("有没过的就报几分之几，并说明已经发给 Agent", () => {
    expect(
      verificationStatus({
        failed: 1,
        skipped: [],
        repairPrompt: "fix it",
        results: [check(0), check(1)],
      })
    ).toEqual({ kind: "message", key: "tasks.verify.failed", params: { failed: 1, total: 2 } });
  });

  /** 放弃时第几轮停的、为什么停都要带上：否则用户只看到工作区变了 */
  it("修复放弃时带着轮数和原因", () => {
    expect(
      repairStatus({
        iterations: 2,
        stopReason: "iteration budget exhausted",
        checksFailed: true,
        results: [check(1)],
      })
    ).toEqual({
      kind: "message",
      key: "tasks.repair.gaveUp",
      params: { rounds: 2, reason: "iteration budget exhausted" },
    });
  });

  it("修好了也是同一组参数，只换键", () => {
    expect(
      repairStatus({
        iterations: 1,
        stopReason: "checks passed",
        checksFailed: false,
        results: [check(0)],
      })
    ).toEqual({
      kind: "message",
      key: "tasks.repair.passed",
      params: { rounds: 1, reason: "checks passed" },
    });
  });

  /**
   * 后端的拒绝理由原样上屏。它是"去切成 Auto 模式"这件事的唯一说明，
   * 套进我们的模板或者兜成一句"修复失败"，用户就不知道该做什么了。
   */
  it("后端说什么就显示什么", () => {
    expect(backendStatus("requires Auto mode")).toEqual({
      kind: "raw",
      text: "requires Auto mode",
    });
    expect(backendStatus(new Error("spawn failed"))).toEqual({
      kind: "raw",
      text: "spawn failed",
    });
  });
});
