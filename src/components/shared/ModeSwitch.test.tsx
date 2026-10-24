// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import ModeSwitch from "./ModeSwitch";
import { useLocaleStore } from "../../i18n";

// 没有 setup 文件，RTL 的自动清理不存在：两个渲染测试共用一个 document 就会互相看见
afterEach(() => {
  cleanup();
  useLocaleStore.getState().setLocale("en");
});

describe("ModeSwitch", () => {
  it("labels both modes in English", () => {
    useLocaleStore.getState().setLocale("en");
    render(<ModeSwitch mode="suggest" onChange={() => {}} />);
    expect(screen.getByRole("radio", { name: "Suggest" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Auto" })).toBeTruthy();
  });

  /**
   * 这个组件以前是英文按钮配中文 tooltip —— 界面上第一处中英混排，而且没有任何机制
   * 能发现它。现在语言是一个变量，两种语言都要是完整的一套。
   */
  it("labels both modes in Chinese", () => {
    useLocaleStore.getState().setLocale("zh");
    render(<ModeSwitch mode="auto" onChange={() => {}} />);
    const auto = screen.getByRole("radio", { name: "自动" });
    expect(auto.getAttribute("aria-checked")).toBe("true");
    expect(auto.getAttribute("title")).toBe("改动直接落盘，可以一键撤销");
    expect(screen.getByRole("radio", { name: "待审查" })).toBeTruthy();
    // 分组名也要跟着换：读屏用户听到的不该是半英半中
    expect(screen.getByRole("radiogroup", { name: "Agent 模式" })).toBeTruthy();
  });
});
