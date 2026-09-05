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

  it("keeps ask fully manual and only widens create for suggest", () => {
    expect(permissionsForPreset("ask")).toEqual({
      allowFileCreate: false,
      allowFileDelete: false,
      allowCommandRun: false,
      allowGitActions: false,
    });
    // suggest 放开新建文件，但不放开命令执行 —— MCP 工具策略依赖这一点
    expect(permissionsForPreset("suggest").allowFileCreate).toBe(true);
    expect(permissionsForPreset("suggest").allowCommandRun).toBe(false);
  });

  it("returns a fresh object so callers cannot mutate the shared presets", () => {
    const permissions = permissionsForPreset("ask");
    permissions.allowFileDelete = true;

    expect(DEFAULT_PERMISSIONS.allowFileDelete).toBe(false);
    expect(permissionsForPreset("ask").allowFileDelete).toBe(false);
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

  it("ignores the file and git toggles", () => {
    // MCP 工具是外部进程执行，只应跟随 allowCommandRun；
    // 放开文件或 git 权限不应顺带放开任意外部工具。
    expect(
      mcpApprovalForPermissions({
        allowFileCreate: true,
        allowFileDelete: true,
        allowCommandRun: false,
        allowGitActions: true,
      })
    ).toBe("auto_approved_only");

    expect(
      mcpApprovalForPermissions({
        allowFileCreate: false,
        allowFileDelete: false,
        allowCommandRun: true,
        allowGitActions: false,
      })
    ).toBe("allow_all");
  });
});
