import { describe, expect, it } from "vitest";
import {
  CREATE_FILES_PERMISSIONS,
  READ_ONLY_PERMISSIONS,
  RUN_COMMANDS_PERMISSIONS,
  mcpApprovalForPermissions,
  permissionsForPreset,
  type AgentPermissionPreset,
} from "./agent";

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
