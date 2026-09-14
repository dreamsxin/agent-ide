import { describe, expect, it } from "vitest";
import {
  attachLoadedChildren,
  loadedDirectoryPaths,
  type ExplorerNode,
} from "./explorerTree";

function dir(path: string, children?: ExplorerNode[], loaded = children != null): ExplorerNode {
  return {
    id: path,
    name: path.split("/").pop() ?? path,
    path,
    isDir: true,
    size: 0,
    childrenLoaded: loaded,
    children: children ?? [],
  };
}

function file(path: string): ExplorerNode {
  return {
    id: path,
    name: path.split("/").pop() ?? path,
    path,
    isDir: false,
    size: 1,
  };
}

describe("loadedDirectoryPaths", () => {
  it("finds expanded directories at every depth", () => {
    const tree = [
      dir("src", [dir("src/components", [file("src/components/App.tsx")]), file("src/main.ts")]),
      dir("docs", undefined, false),
      file("README.md"),
    ];

    expect(loadedDirectoryPaths(tree)).toEqual(["src", "src/components"]);
  });

  /** 没展开过的目录不能出现：把它当成"已加载但空"会让待展开的目录变成空目录 */
  it("ignores directories that were never expanded", () => {
    expect(loadedDirectoryPaths([dir("src", undefined, false), file("a.ts")])).toEqual([]);
  });
});

describe("attachLoadedChildren", () => {
  /**
   * 这是整件事的要点：新建/删除/重命名/粘贴之后 Explorer 会重新列根目录，拿到的是
   * 一批 `childrenLoaded: false` 的顶层节点。如果不把展开过的子树接回去，那些目录
   * 在 react-arborist 眼里还是开着的，但内容空了 —— 用户看到"打开却是空的文件夹"。
   */
  it("reattaches a reloaded subtree so an expanded folder is not left empty", () => {
    const fresh = [dir("src", undefined, false), file("README.md")];
    const reloaded = new Map([["src", [file("src/main.ts"), file("src/new.ts")]]]);

    const merged = attachLoadedChildren(fresh, reloaded);

    expect(merged[0].childrenLoaded).toBe(true);
    expect(merged[0].children?.map((node) => node.path)).toEqual([
      "src/main.ts",
      "src/new.ts",
    ]);
    // 新建的文件出现在里面：这就是"重新列一遍"而不是"缓存旧子树"的理由
    expect(merged[0].children?.some((node) => node.path === "src/new.ts")).toBe(true);
  });

  it("reattaches nested levels too", () => {
    const fresh = [dir("src", undefined, false)];
    const reloaded = new Map([
      ["src", [dir("src/components", undefined, false)]],
      ["src/components", [file("src/components/App.tsx")]],
    ]);

    const merged = attachLoadedChildren(fresh, reloaded);
    const components = merged[0].children?.[0];

    expect(components?.childrenLoaded).toBe(true);
    expect(components?.children?.[0].path).toBe("src/components/App.tsx");
  });

  /** 目录在这次操作里被删掉时它不在 fresh 里，剩下的节点不能受影响 */
  it("leaves directories alone when there is nothing to reattach", () => {
    const fresh = [dir("src", undefined, false), file("a.ts")];

    const merged = attachLoadedChildren(fresh, new Map());

    expect(merged[0].childrenLoaded).toBe(false);
    expect(merged[0].children).toEqual([]);
    expect(merged[1]).toBe(fresh[1]);
  });
});
