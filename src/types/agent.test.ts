import { describe, expect, it } from "vitest";
import {
  CREATE_FILES_PERMISSIONS,
  READ_ONLY_PERMISSIONS,
  RUN_COMMANDS_PERMISSIONS,
  describeRunUsage,
  mcpApprovalForPermissions,
  normalizeRunUsage,
  permissionsForPreset,
  type AgentPermissionPreset,
  type RunUsage,
} from "./agent";
import { formatMicrosUsd } from "../utils/money";

describe("permissionsForPreset", () => {
  it("maps each preset to its permission table", () => {
    expect(permissionsForPreset("read-only")).toEqual(READ_ONLY_PERMISSIONS);
    expect(permissionsForPreset("create-files")).toEqual(CREATE_FILES_PERMISSIONS);
    expect(permissionsForPreset("run-commands")).toEqual(RUN_COMMANDS_PERMISSIONS);
  });

  it("is a ladder: each preset adds exactly one grant", () => {
    expect(permissionsForPreset("read-only")).toEqual({
      allowFileCreate: false,
      allowCommandRun: false,
    });
    // create-files 放开新建文件，但不放开命令执行 —— MCP 工具策略依赖这一点
    expect(permissionsForPreset("create-files").allowFileCreate).toBe(true);
    expect(permissionsForPreset("create-files").allowCommandRun).toBe(false);
    expect(permissionsForPreset("run-commands").allowCommandRun).toBe(true);
  });

  it("returns a fresh object so callers cannot mutate the shared presets", () => {
    const permissions = permissionsForPreset("read-only");
    permissions.allowFileCreate = true;

    expect(READ_ONLY_PERMISSIONS.allowFileCreate).toBe(false);
    expect(permissionsForPreset("read-only").allowFileCreate).toBe(false);
  });
});

describe("mcpApprovalForPermissions", () => {
  it("only grants allow_all when command execution is permitted", () => {
    const presets: AgentPermissionPreset[] = ["read-only", "create-files", "run-commands"];
    const approvals = presets.map((preset) =>
      mcpApprovalForPermissions(permissionsForPreset(preset))
    );

    expect(approvals).toEqual(["auto_approved_only", "auto_approved_only", "allow_all"]);
  });

  it("ignores the file-creation toggle", () => {
    // MCP 工具是外部进程执行，只应跟随 allowCommandRun；
    // 放开新建文件不应顺带放开任意外部工具。
    expect(
      mcpApprovalForPermissions({
        allowFileCreate: true,
        allowCommandRun: false,
      })
    ).toBe("auto_approved_only");

    expect(
      mcpApprovalForPermissions({
        allowFileCreate: false,
        allowCommandRun: true,
      })
    ).toBe("allow_all");
  });
});

describe("run usage", () => {
  function usage(overrides: Partial<RunUsage> = {}): RunUsage {
    return {
      totalTokens: 1_500,
      maxTotalTokens: null,
      spendMicros: 2_000,
      maxSpendMicros: null,
      calls: 3,
      reportedCalls: 3,
      ...overrides,
    };
  }

  /**
   * 状态栏和 action log 里的同一笔花费必须是同一个字符串，所以这里的格式化
   * 逐位复制后端 `format_micros_usd`（截断到 4 位小数，不四舍五入）。
   */
  it("formats spend the same way the backend does", () => {
    expect(formatMicrosUsd(2_000)).toBe("$0.0020");
    expect(formatMicrosUsd(1_234_567)).toBe("$1.2345");
    expect(formatMicrosUsd(0)).toBe("$0.0000");
  });

  it("says nothing before the first provider call", () => {
    expect(describeRunUsage(usage({ calls: 0, reportedCalls: 0 }), formatMicrosUsd)).toBeNull();
  });

  it("reports unknown rather than zero when the provider never says", () => {
    const described = describeRunUsage(
      usage({ totalTokens: 0, reportedCalls: 0, spendMicros: 0 }),
      formatMicrosUsd
    );

    // 打印 0 会让人以为这次运行免费
    expect(described?.label).toBe("usage unknown");
    expect(described?.detail).toContain("cannot be estimated");
  });

  it("calls out a partial report, because the cap undercounts there", () => {
    const described = describeRunUsage(usage({ reportedCalls: 1 }), formatMicrosUsd);

    expect(described?.label).toBe("1500 tok · $0.0020");
    expect(described?.detail).toContain("lower bound");
    expect(described?.detail).toContain("undercounts");
  });

  it("distinguishes an uncomputable cost from a free run", () => {
    const described = describeRunUsage(usage({ spendMicros: null }), formatMicrosUsd);

    expect(described?.label).toBe("1500 tok");
    expect(described?.detail).toContain("not computable");
  });
});

describe("normalizeRunUsage", () => {
  it("returns null for anything that is not a usage object", () => {
    expect(normalizeRunUsage(null)).toBeNull();
    expect(normalizeRunUsage(undefined)).toBeNull();
    expect(normalizeRunUsage("1500")).toBeNull();
  });

  /**
   * 事件里少字段就当 0 / null，而不是让 undefined 一路漏到 `toFixed` 变成 NaN。
   * 旧版本后端不带 `usage` 字段时也走这条路。
   */
  it("fills missing counters instead of letting undefined through", () => {
    const normalized = normalizeRunUsage({ totalTokens: 10 });

    expect(normalized).toEqual({
      totalTokens: 10,
      maxTotalTokens: null,
      spendMicros: null,
      maxSpendMicros: null,
      calls: 0,
      reportedCalls: 0,
    });
  });
});

