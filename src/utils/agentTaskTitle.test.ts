import { describe, expect, it } from "vitest";
import { deriveTaskTitle } from "./agentTaskTitle";

describe("deriveTaskTitle", () => {
  it("takes the first non-empty line, not the whole prompt", () => {
    const title = deriveTaskTitle("\n  Add pagination to the users list\n\n只改后端，不动 UI\n");
    expect(title).toBe("Add pagination to the users list");
  });

  it("collapses whitespace so pasted text does not leave gaps in the header", () => {
    expect(deriveTaskTitle("Fix\tthe   parser")).toBe("Fix the parser");
  });

  it("truncates a long line to 60 chars including the ellipsis", () => {
    const title = deriveTaskTitle("x".repeat(200));
    expect(title).toHaveLength(60);
    expect(title.endsWith("...")).toBe(true);
  });

  /** 按码元切会把 emoji 劈成半个代理对，标题栏里显示成一个 U+FFFD */
  it("never splits an astral character at the cut", () => {
    const title = deriveTaskTitle(`${"x".repeat(56)}😀 tail`);
    expect(title.endsWith("...")).toBe(true);
    expect(title).not.toContain("\uFFFD");
    // 半个代理对在字符串里就是一个孤立的 high surrogate
    expect([...title].some((ch) => ch.charCodeAt(0) >= 0xd800 && ch.charCodeAt(0) <= 0xdbff && ch.length === 1)).toBe(
      false
    );
  });


  it("returns an empty string rather than inventing a title", () => {
    expect(deriveTaskTitle("   \n\t\n")).toBe("");
    expect(deriveTaskTitle("")).toBe("");
  });
});
