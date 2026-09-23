import { describe, expect, it } from "vitest";
import type { DiffHunk } from "../../types/agent";
import { hunkBanner, hunkKind } from "./diffPresentation";

function hunk(overrides: Partial<DiffHunk>): DiffHunk {
  return {
    oldStart: 1,
    oldLines: 1,
    newStart: 1,
    newLines: 1,
    content: "",
    original: "",
    updated: "",
    ...overrides,
  };
}

describe("hunkKind", () => {
  it("只有新内容是新建", () => {
    expect(hunkKind(hunk({ updated: "hello\n" }))).toBe("created");
  });

  it("两边都有内容是修改", () => {
    expect(hunkKind(hunk({ original: "a\n", updated: "b\n" }))).toBe("modified");
  });

  /**
   * 这条是这个模块存在的理由：删除记录的 `content` 是空字符串，原来会掉到兜底分支
   * 去 split 它，于是删掉的内容一行都看不到。
   */
  it("只有旧内容且是删除操作时，不是兜底分支", () => {
    expect(hunkKind(hunk({ original: "goodbye\n" }), "delete")).toBe("deleted");
  });

  it("同样的形状但操作不是删除，说的是清空而不是删文件", () => {
    expect(hunkKind(hunk({ original: "goodbye\n" }), "edit")).toBe("emptied");
    // 直接工具写回的 diff 才带 operation；模型给的 diff 可能没有
    expect(hunkKind(hunk({ original: "goodbye\n" }))).toBe("emptied");
  });

  it("两边都空只能讲统一 diff 文本", () => {
    expect(hunkKind(hunk({ content: "@@ -1 +1 @@\n-a\n+b\n" }))).toBe("raw");
  });

  // 只有空白的内容不算内容：一个全是空格的 `updated` 不该被当成"新建了一个文件"
  it("只有空白不算有内容", () => {
    expect(hunkKind(hunk({ original: "a\n", updated: "   \n" }), "edit")).toBe("emptied");
  });

  /**
   * 纯移动两边都没有内容（内容一个字节没变），落到兜底分支会渲染出一个空行，
   * 于是"从哪搬来的"完全看不到 —— 而那正是这次改动的全部内容。
   */
  it("纯移动是自己一档，不是兜底", () => {
    expect(hunkKind(hunk({}), "move")).toBe("moved");
  });

  it("移动之后又改了内容，就按修改画，横幅另说", () => {
    expect(hunkKind(hunk({ original: "a\n", updated: "b\n" }), "move")).toBe("modified");
  });
});

describe("hunkBanner", () => {
  it("移动的横幅必须带上源路径：卡片标题只有落点", () => {
    expect(hunkBanner("moved", "src/old.ts")).toEqual({
      key: "diff.banner.movedFrom",
      params: { path: "src/old.ts" },
    });
  });

  it("源路径缺失时也要说清这是一次移动，而不是显示 undefined", () => {
    expect(hunkBanner("moved", null)).toEqual({ key: "diff.banner.moved" });
  });

  it("删除和清空的说法不能混：后者文件还在", () => {
    expect(hunkBanner("deleted")).not.toEqual(hunkBanner("emptied"));
  });

  it("左右对照和统一 diff 文本没有横幅", () => {
    expect(hunkBanner("modified")).toBeNull();
    expect(hunkBanner("raw")).toBeNull();
  });
});
