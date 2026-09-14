import { describe, expect, it } from "vitest";
import {
  describeExternalAction,
  isFromOtherRun,
  isRefusedAction,
  normalizeExternalActions,
  summarizeExternalActions,
} from "./externalActions";

describe("normalizeExternalActions", () => {
  it("keeps a record whose fields are all present", () => {
    const records = normalizeExternalActions([
      {
        id: "a-1",
        timestamp: "2026-09-14T00:00:00Z",
        kind: "browser_open",
        target: "https://example.com/docs",
        detail: 'Opened "Docs" (tab 1)',
        runId: "run-7",
      },
    ]);

    expect(records).toEqual([
      {
        id: "a-1",
        timestamp: "2026-09-14T00:00:00Z",
        kind: "browser_open",
        target: "https://example.com/docs",
        detail: 'Opened "Docs" (tab 1)',
        runId: "run-7",
      },
    ]);
  });

  it("keeps a partially filled record instead of dropping it", () => {
    // 撤不回的动作只有这一份记录，缺 detail 也要显示出来
    const records = normalizeExternalActions([{ kind: "browser_open", target: "https://a.example" }]);

    expect(records).toHaveLength(1);
    expect(records[0].detail).toBe("");
    expect(records[0].runId).toBeNull();
    // id 缺失时补一个稳定的，否则同一批记录会在 React 里互相顶掉
    expect(records[0].id).toBe("external-0");
  });

  it("drops only entries that would render as a blank line", () => {
    expect(normalizeExternalActions([{ detail: "no kind, no target" }, null, 42])).toEqual([]);
  });

  it("returns an empty list for anything that is not an array", () => {
    expect(normalizeExternalActions(undefined)).toEqual([]);
    expect(normalizeExternalActions({ kind: "browser_open" })).toEqual([]);
  });
});

describe("isRefusedAction / summarizeExternalActions", () => {
  const actions = normalizeExternalActions([
    { id: "1", kind: "browser_open", target: "https://ok.example" },
    { id: "2", kind: "browser_open_refused", target: "https://evil.example" },
    { id: "3", kind: "browser_tabs_failed", target: "127.0.0.1:9222" },
  ]);

  it("counts refusals and failures apart from what actually happened", () => {
    // 这是标题里唯一要区分的事：真的出网了几次，被挡了几次
    expect(summarizeExternalActions(actions)).toEqual({ performed: 1, refused: 2 });
    expect(actions.filter(isRefusedAction).map((action) => action.id)).toEqual(["2", "3"]);
  });

  it("puts the target first and only appends a detail when there is one", () => {
    expect(describeExternalAction(actions[0])).toBe("https://ok.example");
    expect(
      describeExternalAction({ ...actions[0], detail: "Opened it" })
    ).toBe("https://ok.example — Opened it");
  });
});

describe("isFromOtherRun", () => {
  const action = normalizeExternalActions([
    { id: "1", kind: "browser_open", target: "https://a.example", runId: "run-1" },
  ])[0];

  it("marks a record whose run differs from the one on screen", () => {
    // 后端跨运行保留记录，所以界面必须说清哪些不是这次干的
    expect(isFromOtherRun(action, "run-2")).toBe(true);
    expect(isFromOtherRun(action, "run-1")).toBe(false);
  });

  it("says nothing when either side has no run id", () => {
    // 标错来源比不标更糟
    expect(isFromOtherRun(action, null)).toBe(false);
    expect(isFromOtherRun({ ...action, runId: null }, "run-2")).toBe(false);
  });
});
