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
export type HunkKind = "created" | "deleted" | "emptied" | "modified" | "raw";

export function hunkKind(hunk: DiffHunk, operation?: string | null): HunkKind {
  const hasOriginal = hunk.original.trim().length > 0;
  const hasUpdated = hunk.updated.trim().length > 0;
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

/** 横幅文字；`modified` 走左右对照，没有单行横幅 */
export const HUNK_BANNER: Record<Exclude<HunkKind, "modified" | "raw">, string> = {
  created: "+ New file",
  deleted: "- Deleted file",
  emptied: "- All content removed",
};
