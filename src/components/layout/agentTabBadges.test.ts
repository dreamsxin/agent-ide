import { describe, expect, it } from "vitest";
import { changesBadge } from "./agentTabBadges";

describe("changesBadge", () => {
  it("counts pending changes when there are any", () => {
    expect(changesBadge(3, 0)).toEqual({
      text: "3",
      tone: "pending",
      hint: "3 change(s) waiting for review",
    });
  });

  it("still marks the tab when a run only acted outside the workspace", () => {
    // 这是这个函数存在的理由：只做了外部动作的运行不产生待审 diff，角标只数 diff 的
    // 版本里它在界面上没有任何痕迹，而那份记录是它唯一的补偿
    expect(changesBadge(0, 2)).toEqual({
      text: "2",
      tone: "external",
      hint: "2 external action(s) that cannot be undone",
    });
  });

  it("does not merge the two counts into one number", () => {
    // 待审改动是"你可以决定"，外部动作是"已经发生"；同一个数字会把两件事说成一件
    const badge = changesBadge(2, 5);
    expect(badge?.text).toBe("2");
    expect(badge?.tone).toBe("pending");
  });

  it("shows nothing when there is nothing to say", () => {
    expect(changesBadge(0, 0)).toBeNull();
  });
});
