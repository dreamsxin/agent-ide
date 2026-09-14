/**
 * 文件树的纯逻辑，从 `Explorer.tsx` 里拆出来 —— 那个组件要 Tauri 桥、
 * ResizeObserver 和 react-arborist 才能渲染，塞在里面的判断没法单独测。
 */

export interface ExplorerNode {
  id: string;
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  /** 这个目录的子节点是否已经列过；没列过时 `children` 为空只是"还不知道" */
  childrenLoaded?: boolean;
  children?: ExplorerNode[];
}

/**
 * 收集所有已经展开（列过子节点）的目录路径，含嵌套，深度优先。
 *
 * 用来做什么：任何一次新建 / 删除 / 重命名 / 粘贴都会让 Explorer 重新列根目录。
 * 在此之前重新列会把整棵 `rootData` 换成一批 `childrenLoaded: false` 的顶层节点，
 * 于是所有展开过的子树凭空消失 —— 而 react-arborist 的展开状态存在它自己内部、
 * 按 id 记，它仍然认为那些目录是开着的。结果就是"目录开着但里面是空的"，
 * 用户得来回折叠两次才能看回来。有了这份清单就能在重新列根之后把它们再列一遍。
 */
export function loadedDirectoryPaths(nodes: ExplorerNode[]): string[] {
  const paths: string[] = [];
  for (const node of nodes) {
    if (!node.isDir) continue;
    if (node.childrenLoaded) paths.push(node.path);
    if (node.children?.length) paths.push(...loadedDirectoryPaths(node.children));
  }
  return paths;
}

/**
 * 把重新列出来的子节点按路径接回新树。
 *
 * 只认 `childrenByPath` 里有的目录：清单里没有的目录保持"未展开"，而不是被塞一个
 * 空数组 —— 空数组和"还没列过"在界面上是两种不同的东西（空目录 vs 待展开）。
 * 目录在这次操作里被删掉时它压根不在 `fresh` 里，自然也就不会被接上。
 */
export function attachLoadedChildren(
  fresh: ExplorerNode[],
  childrenByPath: Map<string, ExplorerNode[]>
): ExplorerNode[] {
  return fresh.map((node) => {
    if (!node.isDir) return node;
    const children = childrenByPath.get(node.path);
    if (!children) return node;
    return {
      ...node,
      children: attachLoadedChildren(children, childrenByPath),
      childrenLoaded: true,
    };
  });
}

/**
 * 粘贴时依次尝试的名字：原名，然后 `foo Copy.ts`、`foo Copy 2.ts`……
 *
 * **原名必须排在第一个。** 调用方按顺序试，直到后端不再报"目标已存在"，所以粘到
 * 一个还没有同名文件的目录里就应该保留原名 —— 这是所有文件管理器的行为。之前这个
 * 列表从 ` Copy` 开始，于是把 `foo.ts` 粘到另一个目录也会变成 `foo Copy.ts`，
 * 用户还得再重命名一次。
 *
 * 扩展名按**最后**一个点切分，且 `dotIndex > 0` 让 `.gitignore` 整体当作主干
 * （不会变成 ` Copy.gitignore`）。`a.tar.gz` 会切成 `a.tar Copy.gz`，这是这种切法
 * 的已知代价，换成识别复合扩展名要维护一张清单，不值得。
 */
export function copyNameCandidates(sourceName: string, limit = 50): string[] {
  const dotIndex = sourceName.lastIndexOf(".");
  const hasExtension = dotIndex > 0;
  const stem = hasExtension ? sourceName.slice(0, dotIndex) : sourceName;
  const ext = hasExtension ? sourceName.slice(dotIndex) : "";
  return [
    sourceName,
    ...Array.from(
      { length: limit },
      (_, index) => `${stem}${index === 0 ? " Copy" : ` Copy ${index + 1}`}${ext}`
    ),
  ];
}

/** Windows 上被设备名占用的名字，带扩展名也一样不能用 */
const WINDOWS_RESERVED = new Set([
  "CON",
  "PRN",
  "AUX",
  "NUL",
  ...Array.from({ length: 9 }, (_, i) => `COM${i + 1}`),
  ...Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`),
]);

/**
 * 校验新建 / 重命名对话框里输入的名字，返回给用户看的原因，合法则返回 `null`。
 *
 * 之前这里只检查"非空"，输入的字符串直接进 `joinPath`。于是 `a/b/c` 会静默地建出
 * 一层嵌套路径，`../x` 会跑到父目录去 —— 用户以为自己在给一个文件起名字，实际上
 * 写了一段路径。这些不是安全漏洞（后端仍然把路径夹在工作区内），但结果和用户的
 * 意图不符，而且事后很难看出发生了什么。
 *
 * 刻意**不查同名**：同名的正确结果后端已经给了（拒绝已存在的目标），而在前端按
 * 大小写不敏感去查会在 Linux 上误拒 —— 那里 `README.md` 和 `readme.md` 是两个文件。
 * 这里只挡没有后备的那些情况：形状本身就不是一个名字。
 */
export function validateEntryName(rawName: string): string | null {
  const name = rawName.trim();
  if (!name) return "Name cannot be empty.";
  if (name === "." || name === "..") return `"${name}" is not a name.`;
  if (/[/\\]/.test(name)) {
    return "Name cannot contain a path separator. Create the folder first, then the file inside it.";
  }
  if (name.includes(":")) return "Name cannot contain ':'.";
  if (/["<>|?*]/.test(name)) return 'Name cannot contain any of " < > | ? *';
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f]/.test(name)) return "Name cannot contain control characters.";
  // Windows 会静默地把结尾的点去掉，于是拿到的文件名和输入的不是一个
  if (name.endsWith(".")) return "Name cannot end with '.'.";
  const stem = name.split(".")[0].toUpperCase();
  if (WINDOWS_RESERVED.has(stem)) {
    return `"${stem}" is reserved by Windows and cannot be used as a name.`;
  }
  return null;
}

