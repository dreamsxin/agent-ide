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

  it("returns an empty string rather than inventing a title", () => {
    expect(deriveTaskTitle("   \n\t\n")).toBe("");
    expect(deriveTaskTitle("")).toBe("");
  });
});
