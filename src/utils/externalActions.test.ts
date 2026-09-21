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
        restored: false,
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
    { id: "4", kind: "browser_open_cancelled", target: "https://late.example" },
  ]);

  it("counts refusals, failures and Stop-cancelled attempts apart from what actually happened", () => {
    // 这是标题里唯一要区分的事：真的出网了几次，没成的几次
    expect(summarizeExternalActions(actions)).toEqual({ performed: 1, refused: 3 });
    expect(actions.filter(isRefusedAction).map((action) => action.id)).toEqual(["2", "3", "4"]);
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

describe("restored records", () => {
  it("only trusts an explicit true, so an older backend's records read as this session's", () => {
    // 后端不给这个字段时当成"这次会话的"：把刚刚发生的事标成历史，用户会以为它没发生
    const [current] = normalizeExternalActions([
      { id: "1", kind: "browser_open", target: "https://a.example" },
    ]);
    expect(current.restored).toBe(false);

    const [truthy] = normalizeExternalActions([
      { id: "2", kind: "browser_open", target: "https://a.example", restored: "yes" },
    ]);
    expect(truthy.restored).toBe(false);

    const [restored] = normalizeExternalActions([
      { id: "3", kind: "browser_open", target: "https://a.example", restored: true },
    ]);
    expect(restored.restored).toBe(true);
  });

  /**
   * 恢复出来的记录照旧算进"发生过几次"。它们真的发生了，而且撤不回 —— 这正是把它们
   * 落盘的理由，把它们从计数里排除等于又一次把它们藏起来。
   */
  it("still counts towards what actually happened", () => {
    const actions = normalizeExternalActions([
      { id: "1", kind: "browser_open", target: "https://a.example", restored: true },
      { id: "2", kind: "browser_open_refused", target: "https://b.example", restored: true },
    ]);

    expect(summarizeExternalActions(actions)).toEqual({ performed: 1, refused: 1 });
  });
});
