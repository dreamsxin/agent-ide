import type { BrowserTab } from "../types/browser";
import type { MessageKey } from "../i18n/messages";

/** 给用户看的一句话，用文案键加参数表示；拼串会把英文语序写进这两个判断里 */
export interface BrowserMessage {
  key: MessageKey;
  params?: Record<string, string | number>;
}

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
  error?: BrowserMessage;
} {
  const trimmed = (selection ?? "").trim();
  if (!trimmed) {
    return { error: { key: "browser.selection.empty" } };
  }
  if (/\s/.test(trimmed)) {
    return { error: { key: "browser.selection.notUrl" } };
  }
  return { url: trimmed };
}

/** 标签页列表的一句话摘要；标题可能很长，也可能一个都没有 */
export function describeTabs(tabs: BrowserTab[], maxTitles = 3): BrowserMessage {
  if (tabs.length === 0) {
    return { key: "browser.tabs.none" };
  }
  const titles = tabs
    .slice(0, maxTitles)
    .map((tab) => (tab.title.trim() ? tab.title.trim() : tab.url))
    // 一个标题能有整段句子那么长，列表读起来就没用了
    .map((title) => (title.length > 60 ? `${title.slice(0, 57)}...` : title))
    .join(", ");
  const extra = tabs.length - maxTitles;
  if (extra > 0) {
    return { key: "browser.tabs.manyMore", params: { count: tabs.length, titles, extra } };
  }
  // 数量词的位置和"个"这种量词只有整句才管得了，所以单复数各一条键
  return {
    key: tabs.length === 1 ? "browser.tabs.one" : "browser.tabs.many",
    params: { count: tabs.length, titles },
  };
}
