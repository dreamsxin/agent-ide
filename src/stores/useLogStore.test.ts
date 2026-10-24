import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LogEntry } from "../types/project";

function createFakeStorage() {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
    clear: () => void store.clear(),
  };
}

const fakeStorage = createFakeStorage();

vi.stubGlobal("window", { localStorage: fakeStorage });
vi.stubGlobal("localStorage", fakeStorage);

function setWorkspacePath(path: string | null) {
  if (path === null) {
    fakeStorage.removeItem("agent-ide-workspace-path");
  } else {
    fakeStorage.setItem("agent-ide-workspace-path", path);
  }
}

async function freshStore() {
  return (await import("./useLogStore")).useLogStore;
}

function logEntry(level: LogEntry["level"], message: string): Omit<LogEntry, "id"> {
  return { level, message, time: new Date().toISOString(), source: "system" };
}

describe("useLogStore", () => {
  beforeEach(() => {
    fakeStorage.clear();
    vi.resetModules();
  });

  it("starts empty when nothing is persisted", async () => {
    const useLogStore = await freshStore();
    expect(useLogStore.getState().logs).toEqual([]);
  });

  it("adds logs with monotonic ids and caps the list at 500", async () => {
    const useLogStore = await freshStore();
    for (let i = 0; i < 505; i++) {
      useLogStore.getState().addLog(logEntry("info", `entry-${i}`));
    }

    const logs = useLogStore.getState().logs;
    expect(logs).toHaveLength(500);
    expect(logs[0].message).toBe("entry-5");
    expect(logs[499].message).toBe("entry-504");
  });

  it("persists logs and restores them for the same workspace", async () => {
    setWorkspacePath("D:/work/agent-ide");
    const useLogStore = await freshStore();
    useLogStore.getState().addLog(logEntry("warn", "first"));

    vi.resetModules();
    const reloaded = await freshStore();
    reloaded.getState().restoreLogs("D:/work/agent-ide");

    expect(reloaded.getState().logs.map((log) => log.message)).toEqual(["first"]);
  });

  it("drops persisted logs from a different workspace on restore", async () => {
    setWorkspacePath("D:/work/project-a");
    const useLogStore = await freshStore();
    useLogStore.getState().addLog(logEntry("error", "project-a log"));

    vi.resetModules();
    const reloaded = await freshStore();
    reloaded.getState().restoreLogs("D:/work/project-b");

    expect(reloaded.getState().logs).toEqual([]);
  });

  it("continues id numbering after restoring persisted logs", async () => {
    setWorkspacePath("D:/work/agent-ide");
    const useLogStore = await freshStore();
    useLogStore.getState().addLog(logEntry("info", "before reload"));

    vi.resetModules();
    const reloaded = await freshStore();
    reloaded.getState().restoreLogs("D:/work/agent-ide");
    reloaded.getState().addLog(logEntry("info", "after reload"));

    const messages = reloaded.getState().logs.map((log) => log.message);
    expect(messages).toEqual(["before reload", "after reload"]);
    const ids = reloaded.getState().logs.map((log) => Number(log.id));
    expect(ids[1]).toBeGreaterThan(ids[0]);
  });

  it("clears logs and persists the empty list", async () => {
    setWorkspacePath("D:/work/agent-ide");
    const useLogStore = await freshStore();
    useLogStore.getState().addLog(logEntry("info", "to be cleared"));
    useLogStore.getState().clearLogs();

    expect(useLogStore.getState().logs).toEqual([]);

    vi.resetModules();
    const reloaded = await freshStore();
    expect(reloaded.getState().logs).toEqual([]);
  });

  it("ignores malformed persisted payloads", async () => {
    fakeStorage.setItem("agent-ide-logs", "{not json");
    vi.resetModules();
    const useLogStore = await freshStore();
    expect(useLogStore.getState().logs).toEqual([]);

    fakeStorage.setItem("agent-ide-logs", JSON.stringify({ logs: "nope" }));
    vi.resetModules();
    const reloaded = await freshStore();
    expect(reloaded.getState().logs).toEqual([]);
  });
});
