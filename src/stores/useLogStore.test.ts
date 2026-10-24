import { describe, expect, it } from "vitest";
import { shouldRecordOnDisk } from "./useLogStore";

/**
 * 真机日志（`~/.agent-ide/logs/agent-ide.log`）里发现的重复：后端发事件时已经落盘，
 * 而 `useAgentBridge` 把同一个事件搬进日志面板后，前端又写了一次。文件里因此出现
 * 逐字相同的 `[info] prompt ...` 和 `[ui:info] agent ...`。
 */
describe("shouldRecordOnDisk", () => {
  it("skips the copies of backend events", () => {
    expect(shouldRecordOnDisk("agent")).toBe(false);
  });

  it("keeps the entries only the frontend knows about", () => {
    // 终端、git、文件系统：后端没有这些的记录，缺了它们磁盘日志就少了排查需要的那半边
    expect(shouldRecordOnDisk("system")).toBe(true);
    expect(shouldRecordOnDisk("git")).toBe(true);
    expect(shouldRecordOnDisk("fs")).toBe(true);
  });
});
