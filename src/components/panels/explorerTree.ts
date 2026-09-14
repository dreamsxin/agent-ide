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
