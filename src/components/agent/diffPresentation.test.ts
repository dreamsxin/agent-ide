import { describe, expect, it } from "vitest";
import type { DiffHunk } from "../../types/agent";
import { hunkKind } from "./diffPresentation";

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
});
