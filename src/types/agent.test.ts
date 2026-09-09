import { describe, expect, it } from "vitest";
import {
  AUTO_PERMISSIONS,
  DEFAULT_PERMISSIONS,
  SUGGEST_PERMISSIONS,
  mcpApprovalForPermissions,
  permissionsForPreset,
  type AgentPermissionPreset,
} from "./agent";

describe("permissionsForPreset", () => {
  it("maps each preset to its permission table", () => {
    expect(permissionsForPreset("ask")).toEqual(DEFAULT_PERMISSIONS);
    expect(permissionsForPreset("suggest")).toEqual(SUGGEST_PERMISSIONS);
    expect(permissionsForPreset("auto")).toEqual(AUTO_PERMISSIONS);
  });

  it("keeps ask read-only and only widens create for suggest", () => {
    expect(permissionsForPreset("ask")).toEqual({
      allowFileCreate: false,
      allowCommandRun: false,
    });
    // suggest 放开新建文件，但不放开命令执行 —— MCP 工具策略依赖这一点
    expect(permissionsForPreset("suggest").allowFileCreate).toBe(true);
    expect(permissionsForPreset("suggest").allowCommandRun).toBe(false);
  });

  it("returns a fresh object so callers cannot mutate the shared presets", () => {
    const permissions = permissionsForPreset("ask");
    permissions.allowFileCreate = true;

    expect(DEFAULT_PERMISSIONS.allowFileCreate).toBe(false);
    expect(permissionsForPreset("ask").allowFileCreate).toBe(false);
  });
});

describe("mcpApprovalForPermissions", () => {
  it("only grants allow_all when command execution is permitted", () => {
    const presets: AgentPermissionPreset[] = ["ask", "suggest", "auto"];
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
