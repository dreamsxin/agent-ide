import { describe, expect, it } from "vitest";
import { microsToUsdInput, spendCapStatus, usdToMicros } from "./money";

describe("usdToMicros", () => {
  /**
   * `Number("0.29") * 1_000_000` 是 289999.99999999994。这些值最终要拿去卡住
   * 一次运行的花费，所以换算必须精确到微美元，不能依赖取整方式。
   */
  it("converts decimals exactly, without floating point drift", () => {
    expect(usdToMicros("0.29")).toBe(290_000);
    expect(usdToMicros("0.28")).toBe(280_000);
    expect(usdToMicros("1")).toBe(1_000_000);
    expect(usdToMicros("1.005")).toBe(1_005_000);
    expect(usdToMicros("0.000001")).toBe(1);
  });

  it("truncates below one micro-dollar instead of rounding up to it", () => {
    expect(usdToMicros("0.0000019")).toBe(1);
  });

  /** 0 和空串都算"没填"：后端也把 0 当作未设置，否则一个手写的 0 会锁死所有运行 */
  it("treats blank, zero, and junk as not configured", () => {
    expect(usdToMicros("")).toBeUndefined();
    expect(usdToMicros("   ")).toBeUndefined();
    expect(usdToMicros("0")).toBeUndefined();
    expect(usdToMicros("0.00")).toBeUndefined();
    expect(usdToMicros("abc")).toBeUndefined();
    expect(usdToMicros("-1")).toBeUndefined();
  });
});

describe("microsToUsdInput", () => {
  it("round-trips a value back to the same input string", () => {
    for (const input of ["0.29", "1.005", "12", "0.000001"]) {
      expect(microsToUsdInput(usdToMicros(input))).toBe(input);
    }
  });

  it("renders nothing when unset, so the field stays empty rather than showing 0", () => {
    expect(microsToUsdInput(undefined)).toBe("");
    expect(microsToUsdInput(0)).toBe("");
  });
});

describe("spendCapStatus", () => {
  it("reports a half-configured price as not enforced", () => {
    // 只有输入价：后端会把花费记成"算不出来"并放行，界面必须说明这一点，
    // 否则用户以为自己已经有了成本保护
    expect(spendCapStatus("0.28", "", "1.00")).toBe("no_price");
    expect(spendCapStatus("", "0.42", "1.00")).toBe("no_price");
  });

  it("is off when no cap is set, whatever the prices are", () => {
    expect(spendCapStatus("0.28", "0.42", "")).toBe("off");
    expect(spendCapStatus("", "", "")).toBe("off");
  });

  it("is active only with both prices and a cap", () => {
    expect(spendCapStatus("0.28", "0.42", "1.00")).toBe("active");
  });
});
