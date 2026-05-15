import { describe, expect, it } from "vitest";
import { normalizeFilePath, pathsEqual } from "./paths";

describe("paths", () => {
  it("normalizes Windows file URIs and encoded paths", () => {
    expect(normalizeFilePath("file:///D:/work/a%20b/src/foo.ts")).toBe("D:/work/a b/src/foo.ts");
  });

  it("normalizes backslashes and leading slash drive paths", () => {
    expect(normalizeFilePath("/D:/repo/src/foo.ts")).toBe("D:/repo/src/foo.ts");
    expect(normalizeFilePath("D:\\repo\\src\\foo.ts")).toBe("D:/repo/src/foo.ts");
  });

  it("compares paths case-insensitively after normalization", () => {
    expect(pathsEqual("D:\\Repo\\A.ts", "d:/repo/a.ts")).toBe(true);
  });
});
