import type { BrowserTab } from "../types/browser";

/**
 * 从编辑器选区里取出一个可以打开的 URL。
 *
 * 在前端就把"选中的是一段代码而不是一个地址"挡掉：整段代码送到后端只会换回一句
 * "Only http:// and https:// URLs are allowed"，用户看不出问题出在选区上。真正的
 * scheme / 凭据 / 控制字符检查在 Rust 那边（`services::browser::normalize_target_url`），
 * 这里只做"这看起来像不像一个 URL"。
 */
export function selectionUrlOrError(selection: string | null | undefined): {
  url?: string;
  error?: string;
} {
  const trimmed = (selection ?? "").trim();
  if (!trimmed) {
    return { error: "Select a URL in the editor first." };
  }
  if (/\s/.test(trimmed)) {
    return { error: "The selection is not a single URL." };
  }
  return { url: trimmed };
}

/** 标签页列表的一句话摘要；标题可能很长，也可能一个都没有 */
export function describeTabs(tabs: BrowserTab[], maxTitles = 3): string {
  if (tabs.length === 0) {
    return "Chrome is attached but has no open pages.";
  }
  const titles = tabs
    .slice(0, maxTitles)
    .map((tab) => (tab.title.trim() ? tab.title.trim() : tab.url))
    // 一个标题能有整段句子那么长，列表读起来就没用了
    .map((title) => (title.length > 60 ? `${title.slice(0, 57)}...` : title));
  const suffix = tabs.length > maxTitles ? `, +${tabs.length - maxTitles} more` : "";
  return `${tabs.length} tab${tabs.length === 1 ? "" : "s"}: ${titles.join(", ")}${suffix}`;
}
