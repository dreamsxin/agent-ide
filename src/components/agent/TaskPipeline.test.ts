import { describe, expect, it } from "vitest";
import { stageOutputState } from "./TaskPipeline";
import type { PipelineStage } from "../../types/agent";
import type { LogEntry } from "../../types/project";

const stage = (status: PipelineStage["status"], pauseBefore = false): PipelineStage => ({
  role: "coder",
  name: "Implement",
  status,
  pauseBefore,
});

const log = (level: LogEntry["level"], extra: Partial<LogEntry> = {}): LogEntry => ({
  id: "l1",
  time: "10:00:00",
  level,
  source: "agent",
  message: "…",
  ...extra,
});

describe("流水线阶段的标签", () => {
  /**
   * 断言落在键上：这个函数决定的是"这一阶段现在是什么情况"，不是那句话怎么说。
   * 之前它返回英文字符串，于是这个判断和英文措辞绑死了。
   */
  it("日志里有 error 就算失败，哪怕阶段还标着在跑", () => {
    expect(stageOutputState(stage("active"), [log("error")]).key).toBe(
      "pipelineView.state.failed"
    );
  });

  it("暂停等批准排在产出改动之前 —— 那是唯一需要人动手的状态", () => {
    expect(
      stageOutputState(stage("paused"), [log("info", { diffSummary: "a.ts" })]).key
    ).toBe("pipelineView.state.waitingApproval");
  });

  it("跑完了并且有改动就说产出了改动，没有就说没产出", () => {
    expect(
      stageOutputState(stage("completed"), [log("info", { diffSummary: "a.ts" })]).key
    ).toBe("pipelineView.state.producedDiff");
    expect(stageOutputState(stage("completed"), [log("info")]).key).toBe(
      "pipelineView.state.noDiff"
    );
  });

  it("在跑和还没开始各是各的", () => {
    expect(stageOutputState(stage("active"), []).key).toBe("pipelineView.state.running");
    expect(stageOutputState(stage("pending"), []).key).toBe("pipelineView.state.pending");
  });
});
