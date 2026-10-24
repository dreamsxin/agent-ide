import { describe, expect, it } from "vitest";
import { normalizeLocale, translate, useLocaleStore } from "./index";
import { EN, ZH } from "./messages";

describe("i18n", () => {
  /**
   * 两张表的键必须完全一致。
   *
   * 这是"半中半英"唯一的防线：类型上 `ZH` 是 `Record<MessageKey, string>`，所以漏键
   * 编译不过；这条测试挡的是**多余的键** —— 英文那边删了一条、中文忘了删，下次有人照着
   * 中文表加文案就会加到一个没人读的键上。
   */
  it("keeps the English and Chinese tables in step", () => {
    expect(Object.keys(ZH).sort()).toEqual(Object.keys(EN).sort());
  });

  /**
   * 中文表里不能残留英文原文：那是"看起来翻过了"的假象。
   *
   * 例外只有专有名词 —— 文件名、产品名翻过去反而找不到对应的东西。例外要列名，
   * 不能靠放宽规则，否则下一条漏翻的也会从这个缝里过去。
   */
  it("has a real Chinese string for every key", () => {
    // 例外只有专有名词：`AGENTS.md` 是文件名，"Agent" 是这个产品里贯穿始终的叫法
    // （中文界面里也写作「Agent 设置」「Agent 面板」），翻成"智能体"反而对不上其他地方。
    const sameOnPurpose = new Set(["chat.context.agentsMd", "palette.group.agent"]);
    const untranslated = Object.keys(EN).filter((key) => {
      const messageKey = key as keyof typeof EN;
      if (sameOnPurpose.has(key)) return false;
      return ZH[messageKey] === EN[messageKey] && /[A-Za-z]{4,}/.test(EN[messageKey]);
    });
    expect(untranslated).toEqual([]);
  });

  it("interpolates parameters", () => {
    expect(translate("en", "topbar.noTask", { kind: "test" })).toBe(
      "No test task discovered"
    );
    expect(translate("zh", "topbar.noTask", { kind: "测试" })).toBe(
      "项目里没找到测试命令"
    );
  });

  /**
   * 语言来自 `localStorage` 和 `navigator.language`，两个都可能是别的版本写的或者根本
   * 不认识的值。断言成 `Locale` 只会让一个不存在的语言一路走到查表那一步。
   */
  it("normalizes anything that arrives from outside", () => {
    expect(normalizeLocale("zh-CN")).toBe("zh");
    expect(normalizeLocale("ZH")).toBe("zh");
    expect(normalizeLocale("zh-Hant-TW")).toBe("zh");
    expect(normalizeLocale("en-US")).toBe("en");
    expect(normalizeLocale("de")).toBe("en");
    expect(normalizeLocale("")).toBe("en");
    expect(normalizeLocale(null)).toBe("en");
    expect(normalizeLocale(undefined)).toBe("en");
  });

  it("switches back and forth with one action", () => {
    const store = useLocaleStore.getState();
    store.setLocale("en");
    useLocaleStore.getState().toggleLocale();
    expect(useLocaleStore.getState().locale).toBe("zh");
    useLocaleStore.getState().toggleLocale();
    expect(useLocaleStore.getState().locale).toBe("en");
  });
});
