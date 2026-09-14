import type { DiffHunk } from "../../types/agent";

/**
 * 一个 hunk 该怎么画。
 *
 * 抽出来是因为原来的判断藏在 `HunkBlock` 的三条 if 里，而其中一条是错的：
 * `original` 有内容、`updated` 为空（删文件、或者把文件清空）会掉到最后那条兜底
 * 分支，去 split `hunk.content` —— 而直接工具写回来的删除记录 `content` 是空字符串。
 * 结果是删除卡片渲染出一个空行，删掉的内容一行都不显示，尽管后端完整存着它。
 * 对一个"看得见、撤得回"是存在理由的产品来说，这是最不能接受的那种缺陷。
 */
export type HunkKind = "created" | "deleted" | "emptied" | "modified" | "moved" | "raw";

export function hunkKind(hunk: DiffHunk, operation?: string | null): HunkKind {
  const hasOriginal = hunk.original.trim().length > 0;
  const hasUpdated = hunk.updated.trim().length > 0;
  // 纯移动：内容一个字节都没变，两边自然都是空的。落到兜底分支会渲染出一个空行，
  // 而这次改动的全部内容其实是"路径变了"，横幅就够了。
  if (operation === "move" && !hasOriginal && !hasUpdated) {
    return "moved";
  }
  if (!hasOriginal && hasUpdated) {
    return "created";
  }
  if (hasOriginal && hasUpdated) {
    return "modified";
  }
  if (hasOriginal) {
    // 删掉文件和把文件清空是两件事，横幅不能混：后者文件还在。
    return operation === "delete" ? "deleted" : "emptied";
  }
  // 两边都空：只有 `content`（统一 diff 文本）可讲，保持原来的逐行着色
  return "raw";
}

/** 横幅文字；`modified` 走左右对照，`raw` 只有统一 diff 文本，都没有单行横幅 */
export function hunkBanner(kind: HunkKind, movedFrom?: string | null): string | null {
  switch (kind) {
    case "created":
      return "+ New file";
    case "deleted":
      return "- Deleted file";
    case "emptied":
      return "- All content removed";
    case "moved":
      // 源路径是这张卡片唯一的信息量：没有它就只剩"某个文件被移动了"
      return movedFrom ? `→ Moved from ${movedFrom}` : "→ Moved";
    default:
      return null;
  }
}
