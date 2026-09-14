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
 * 按 id 在整棵树里找节点，含未展开分支下已经列过的子节点。
 *
 * 拖放只把 id 交回来（react-arborist 的 `onMove` 给的是 `dragIds` 和 `parentId`），
 * 而落点判断需要真实路径和"是不是目录"。递归找是必须的：只扫顶层的话，把文件拖进
 * 一个二级目录时会找不到落点，然后静默什么都不做 —— 比报错更糟。
 */
export function findNodeById(nodes: ExplorerNode[], id: string): ExplorerNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.children?.length) {
      const found = findNodeById(node.children, id);
      if (found) return found;
    }
  }
  return null;
}

/**
 * 多选拖动时，去掉那些已经被同批里某个目录包住的节点。
 *
 * 一起选中 `src/` 和 `src/main.ts` 再拖走：目录先搬，`src/main.ts` 跟着一起走了，
 * 第二次 rename 于是打在一个已经不存在的路径上，报一句后端原话，而磁盘其实是对的。
 * 反过来先搬子节点，则会把它从目录里拽出来 —— 用户根本没要求这件事。两种都不该发生。
 */
export function withoutDraggedDescendants(nodes: ExplorerNode[]): ExplorerNode[] {
  const directories = nodes.filter((node) => node.isDir).map((node) => normalizeForCompare(node.path));
  return nodes.filter((node) => {
    const path = normalizeForCompare(node.path);
    return !directories.some((directory) => directory !== path && path.startsWith(`${directory}/`));
  });
}

/** 文件树上一次按键对应的操作，`null` 表示这个组合不归我们管 */
export type ExplorerShortcut = "rename" | "delete" | "copy" | "cut" | "paste";

/**
 * 键盘按下时该做什么。
 *
 * 抽成纯函数是因为这里全是"哪些组合**不**该拦"的判断，而那部分最容易写错：
 * - 带 Alt 的一律放过：那些是操作系统和窗口管理器的地盘，抢过来会让用户的窗口快捷键
 *   在文件树里莫名失效；
 * - `Ctrl+Shift+C` 之类的不认领，返回 null 而不是当成 Copy —— VS Code 里那是另一个
 *   命令，把它悄悄映射成复制比不支持更糟；
 * - Delete 和 Backspace 都算删除：Mac 键盘上没有独立的 Delete 键。
 *
 * react-arborist 自己只在 `onDelete` 存在时处理 Backspace，我们不传那个 handler，
 * 所以这里认领它不会造成两次删除。
 */
export function explorerShortcut(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}): ExplorerShortcut | null {
  if (event.altKey) return null;
  const modified = event.ctrlKey || event.metaKey;

  if (!modified && !event.shiftKey) {
    if (event.key === "F2") return "rename";
    if (event.key === "Delete" || event.key === "Backspace") return "delete";
  }
  if (modified && !event.shiftKey) {
    const key = event.key.toLowerCase();
    if (key === "c") return "copy";
    if (key === "x") return "cut";
    if (key === "v") return "paste";
  }
  return null;
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

/** 比较路径前先统一分隔符：`joinPath` 产出的是 `/`，而 `list_directory` 在 Windows 上给 `\` */
function normalizeForCompare(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

/**
 * 剪切粘贴的目标路径，或者拒绝的原因。
 *
 * 三条拒绝都是"后端也会失败，但话说不清"的情况，提前拦住是为了给一句人看得懂的：
 *   * 搬到自己身上；
 *   * 把一个目录搬进它自己的子目录 —— `fs::rename` 会返回一句裸的 EINVAL；
 *   * 搬到它已经在的那个目录 —— 后端只会说"目标已存在"，读起来像是撞了同名文件，
 *     而实际上什么都不需要做。
 *
 * `reason` 和文案分开给：拖放要区别对待 `sameFolder`（arborist 画的插入线让人以为能
 * 排序，一次无害的手势不该弹错误），而靠匹配错误文案来判断，改一个字就会失效。
 *
 * 大小写按敏感比较：Windows 上文件系统不区分大小写，但在 Linux 上 `A` 和 `a` 是两个
 * 目录，前端按不敏感比会误拒一个合法的移动。真撞上了由后端的"目标已存在"兜住。
 */
export function resolveMoveDestination(
  sourcePath: string,
  sourceName: string,
  targetDirectory: string
): { destination: string } | { error: string; reason: "self" | "descendant" | "sameFolder" } {
  const source = normalizeForCompare(sourcePath);
  const target = normalizeForCompare(targetDirectory);

  if (target === source) {
    return { error: `Cannot move "${sourceName}" into itself.`, reason: "self" };
  }
  if (target.startsWith(`${source}/`)) {
    return { error: `Cannot move "${sourceName}" into a folder inside it.`, reason: "descendant" };
  }
  const separator = source.lastIndexOf("/");
  const currentParent = separator === -1 ? "" : source.slice(0, separator);
  if (currentParent === target) {
    return { error: `"${sourceName}" is already in this folder.`, reason: "sameFolder" };
  }
  return { destination: `${target}/${sourceName}` };
}

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

