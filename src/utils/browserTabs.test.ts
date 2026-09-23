import { describe, expect, it } from "vitest";
import { describeTabs, selectionUrlOrError } from "./browserTabs";

describe("selectionUrlOrError", () => {
  it("takes a trimmed single-token selection as the URL", () => {
    expect(selectionUrlOrError("  https://example.com/docs \n").url).toBe(
      "https://example.com/docs"
    );
  });

  /**
   * 整段代码送到后端只会换回一句 "Only http:// and https:// URLs are allowed"，
   * 用户看不出问题出在选区上。
   */
  it("refuses a selection that is not a single token", () => {
    expect(selectionUrlOrError("const url = 'https://example.com'").error?.key).toBe(
      "browser.selection.notUrl"
    );
    expect(selectionUrlOrError("https://a.example\nhttps://b.example").error).toBeTruthy();
  });

  it("asks for a selection when there is none", () => {
    expect(selectionUrlOrError("").error?.key).toBe("browser.selection.empty");
    expect(selectionUrlOrError(null).error?.key).toBe("browser.selection.empty");
    expect(selectionUrlOrError("   ").error?.key).toBe("browser.selection.empty");
  });
});

describe("describeTabs", () => {
  const tab = (title: string, url = "https://example.com/") => ({ id: title, title, url });

  it("says so plainly when Chrome is attached but empty", () => {
    expect(describeTabs([]).key).toBe("browser.tabs.none");
  });

  it("counts and lists, with the rest folded into a suffix", () => {
    const summary = describeTabs([tab("A"), tab("B"), tab("C"), tab("D")]);

    expect(summary.key).toBe("browser.tabs.manyMore");
    expect(summary.params).toEqual({ count: 4, titles: "A, B, C", extra: 1 });
  });

  it("singular for one tab", () => {
    expect(describeTabs([tab("Only")]).key).toBe("browser.tabs.one");
  });

  /** 一个标题能有整段句子那么长，列表读起来就没用了 */
  it("truncates a very long title", () => {
    const titles = String(describeTabs([tab("x".repeat(200))]).params?.titles);

    expect(titles).toContain("...");
    expect(titles.length).toBeLessThan(70);
  });

  /** 无标题的页面（还在加载）报 URL，而不是一段空白 */
  it("falls back to the URL when the title is blank", () => {
    expect(describeTabs([tab("   ", "https://loading.example/")]).params?.titles).toBe(
      "https://loading.example/"
    );
  });
});
