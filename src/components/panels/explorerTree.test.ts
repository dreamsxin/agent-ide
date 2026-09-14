import { describe, expect, it } from "vitest";
import {
  attachLoadedChildren,
  copyNameCandidates,
  findNodeById,
  loadedDirectoryPaths,
  resolveMoveDestination,
  validateEntryName,
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

describe("copyNameCandidates", () => {
  /**
   * 原名必须是第一个候选。调用方按顺序试到后端不再报"目标已存在"为止，所以粘到一个
   * 没有同名文件的目录里就该保留原名 —— 之前列表从 " Copy" 开始，把 `foo.ts` 粘到
   * 另一个目录也会变成 `foo Copy.ts`，用户还得再重命名一次。
   */
  it("offers the original name first so a free destination keeps it", () => {
    expect(copyNameCandidates("foo.ts")[0]).toBe("foo.ts");
  });

  it("falls back to Copy, Copy 2 … when the name is taken", () => {
    const candidates = copyNameCandidates("foo.ts", 3);
    expect(candidates).toEqual(["foo.ts", "foo Copy.ts", "foo Copy 2.ts", "foo Copy 3.ts"]);
  });

  /** 点文件的整个名字都是主干，不能变成 " Copy.gitignore" */
  it("keeps a dotfile whole", () => {
    expect(copyNameCandidates(".gitignore", 1)).toEqual([".gitignore", ".gitignore Copy"]);
  });
});

describe("resolveMoveDestination", () => {
  it("moves an item into the target folder under its own name", () => {
    expect(resolveMoveDestination("/w/src/app.ts", "app.ts", "/w/lib")).toEqual({
      destination: "/w/lib/app.ts",
    });
  });

  /** `fs::rename` 也会拒，但返回的是一句裸的 EINVAL，看不出问题在哪 */
  it("refuses to move a folder into itself or into its own subtree", () => {
    const intoItself = resolveMoveDestination("/w/a", "a", "/w/a");
    const intoChild = resolveMoveDestination("/w/a", "a", "/w/a/b");

    expect("error" in intoItself && intoItself.error).toContain("into itself");
    expect("error" in intoChild && intoChild.error).toContain("inside it");
  });

  /**
   * 搬到它已经在的目录：后端只会说"目标已存在"，读起来像撞了同名文件，
   * 而实际上什么都不需要做。
   */
  it("says nothing needs doing when the item is already in the target folder", () => {
    const result = resolveMoveDestination("/w/src/app.ts", "app.ts", "/w/src");
    expect("error" in result && result.error).toContain("already in this folder");
  });

  /** 分隔符要先统一：list_directory 在 Windows 上给的是反斜杠 */
  it("compares paths across separator styles", () => {
    const result = resolveMoveDestination("D:\\w\\a", "a", "D:/w/a/b");
    expect("error" in result && result.error).toContain("inside it");
  });

  /** `ab` 只是名字以 `a` 开头，并不在 `a` 里面 */
  it("does not mistake a similarly named sibling for a descendant", () => {
    expect(resolveMoveDestination("/w/a", "a", "/w/ab")).toEqual({
      destination: "/w/ab/a",
    });
  });
});

describe("validateEntryName", () => {
  it("accepts ordinary names", () => {
    expect(validateEntryName("App.tsx")).toBeNull();
    expect(validateEntryName(".gitignore")).toBeNull();
    expect(validateEntryName("a.tar.gz")).toBeNull();
  });

  /**
   * 这是这个校验存在的理由：之前输入的字符串直接进 `joinPath`，`a/b/c` 会静默建出
   * 一层嵌套路径，`../x` 会跑到父目录。后端仍然把路径夹在工作区内，所以不是越权，
   * 但结果和用户的意图不符，而且事后看不出发生了什么。
   */
  it("rejects anything that is a path rather than a name", () => {
    expect(validateEntryName("a/b")).toContain("path separator");
    expect(validateEntryName("a\\b")).toContain("path separator");
    expect(validateEntryName("..")).not.toBeNull();
    expect(validateEntryName(".")).not.toBeNull();
    expect(validateEntryName("../x")).not.toBeNull();
  });

  it("rejects an empty name", () => {
    expect(validateEntryName("")).not.toBeNull();
    expect(validateEntryName("   ")).not.toBeNull();
  });

  /** Windows 会静默丢掉结尾的点，于是拿到的名字和输入的不是一个 */
  it("rejects a trailing dot", () => {
    expect(validateEntryName("notes.")).not.toBeNull();
  });

  /** `name::$DATA` 这类写法在 NTFS 上指向别的东西 */
  it("rejects a colon", () => {
    expect(validateEntryName("name::$DATA")).not.toBeNull();
  });

  it("rejects Windows device names, with or without an extension", () => {
    expect(validateEntryName("CON")).not.toBeNull();
    expect(validateEntryName("nul.txt")).not.toBeNull();
    expect(validateEntryName("COM1")).not.toBeNull();
    // 只是以设备名开头不算
    expect(validateEntryName("console.ts")).toBeNull();
  });
});

describe("findNodeById", () => {
  const tree = [
    dir("src", [dir("src/components", [file("src/components/App.tsx")]), file("src/main.ts")]),
    file("README.md"),
  ];

  /**
   * 拖放只交回 id。只扫顶层的话，把文件拖进一个二级目录会找不到落点，然后静默什么
   * 都不做 —— 比报错更糟。
   */
  it("finds a node nested below an expanded directory", () => {
    expect(findNodeById(tree, "src/components")?.isDir).toBe(true);
    expect(findNodeById(tree, "src/components/App.tsx")?.name).toBe("App.tsx");
  });

  it("finds a top-level node and returns null for an unknown id", () => {
    expect(findNodeById(tree, "README.md")?.path).toBe("README.md");
    expect(findNodeById(tree, "nowhere.ts")).toBeNull();
  });
});

