import { describe, expect, it } from "vitest";
import { formatTokenCount, parseTokenInput } from "./tokenInput";

describe("parseTokenInput", () => {
  /**
   * 数零是这个输入框唯一真正的失误来源：128000 和 1280000 在 11px 的框里几乎一样，
   * 而后者会让整个预算估算差一个数量级。`k` / `m` 存在就是为了不用数。
   */
  it("accepts the shorthand people actually type", () => {
    expect(parseTokenInput("128000")).toBe(128_000);
    expect(parseTokenInput("128k")).toBe(128_000);
    expect(parseTokenInput("128K")).toBe(128_000);
    expect(parseTokenInput("1m")).toBe(1_000_000);
    expect(parseTokenInput("0.5m")).toBe(500_000);
    expect(parseTokenInput("1.5k")).toBe(1_500);
  });

  /** 从别处粘贴过来的数字常常带千分位 */
  it("ignores grouping characters", () => {
    expect(parseTokenInput("128,000")).toBe(128_000);
    expect(parseTokenInput("1_000_000")).toBe(1_000_000);
    expect(parseTokenInput(" 200 000 ")).toBe(200_000);
  });

  /**
   * 认不出来就是"没设"，不猜。猜出来的一个数字会被当成模型窗口用在预算估算里，
   * 而用户完全不知道自己"设"了什么。
   */
  it("refuses to guess at anything else", () => {
    expect(parseTokenInput("")).toBeUndefined();
    expect(parseTokenInput("abc")).toBeUndefined();
    expect(parseTokenInput("128kb")).toBeUndefined();
    expect(parseTokenInput("-5")).toBeUndefined();
    expect(parseTokenInput("1e6")).toBeUndefined();
  });

  /** 后端把 0 当"没设"，界面必须照这个事实回显，而不是显示成"上限 0" */
  it("treats zero as not set, the way the backend does", () => {
    expect(parseTokenInput("0")).toBeUndefined();
    expect(parseTokenInput("0k")).toBeUndefined();
  });
});

describe("formatTokenCount", () => {
  it("groups digits, because counting zeros is the failure mode", () => {
    expect(formatTokenCount(128_000)).toBe("128,000");
  });

  /** 留空和"正在输入"长得一样；这里要回答的正是"我打的这串被认成了什么" */
  it("says not set instead of showing nothing", () => {
    expect(formatTokenCount(undefined)).toBe("not set");
  });
});
