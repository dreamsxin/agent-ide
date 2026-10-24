import { beforeEach, describe, expect, it } from "vitest";
import { useTaskStore, type ProjectTaskDefinition } from "./useTaskStore";

function task(id: string): ProjectTaskDefinition {
  return {
    id,
    label: id,
    command: `echo ${id}`,
    description: `${id} task`,
    source: "test",
  };
}

function resetStore() {
  useTaskStore.setState({
    lastTask: null,
    pendingTerminalCommands: [],
    pendingTerminalSessionRequests: [],
    terminalOutput: {},
    discoveredTasks: [],
    taskDiscoveryLoading: false,
    taskDiscoveryLoaded: false,
    taskDiscoveryError: null,
    taskRuns: {},
    taskRunHistory: [],
  });
}

describe("useTaskStore", () => {
  beforeEach(resetStore);

  it("records task discovery state without clearing the loaded flag", () => {
    useTaskStore.getState().setDiscoveredTasks([task("build")]);
    useTaskStore.getState().setTaskDiscoveryState(true, "boom", true);

    const state = useTaskStore.getState();
    expect(state.discoveredTasks.map((entry) => entry.id)).toEqual(["build"]);
    expect(state.taskDiscoveryLoading).toBe(true);
    expect(state.taskDiscoveryError).toBe("boom");
    expect(state.taskDiscoveryLoaded).toBe(true);
  });

  it("tracks a task run lifecycle and appends history", () => {
    const store = useTaskStore.getState();
    const runId = store.startTaskRun("test", "npm test", "Test");

    expect(useTaskStore.getState().taskRuns.test).toMatchObject({
      runId,
      taskId: "test",
      status: "running",
    });

    useTaskStore
      .getState()
      .finishTaskRun("test", "success", { exitCode: 0, durationMs: 120, output: "ok" });

    const run = useTaskStore.getState().taskRuns.test;
    expect(run.status).toBe("success");
    expect(run.exitCode).toBe(0);
    expect(run.finishedAt).toBeGreaterThan(0);

    expect(useTaskStore.getState().taskRunHistory).toHaveLength(1);
    expect(useTaskStore.getState().taskRunHistory[0]).toMatchObject({
      runId,
      taskId: "test",
      status: "success",
    });
  });

  it("creates a fallback run entry when finishing an unknown task", () => {
    useTaskStore.getState().finishTaskRun("ghost", "failed", { exitCode: 1 });

    expect(useTaskStore.getState().taskRuns.ghost).toMatchObject({
      taskId: "ghost",
      status: "failed",
      exitCode: 1,
    });
    expect(useTaskStore.getState().taskRunHistory).toHaveLength(1);
  });

  it("caps run history at 100 entries", () => {
    for (let i = 0; i < 105; i++) {
      useTaskStore.getState().finishTaskRun("build", "success", { exitCode: 0 });
    }

    expect(useTaskStore.getState().taskRunHistory).toHaveLength(100);
  });

  it("clears run history", () => {
    useTaskStore.getState().finishTaskRun("build", "success", { exitCode: 0 });
    useTaskStore.getState().clearTaskRunHistory();

    expect(useTaskStore.getState().taskRunHistory).toHaveLength(0);
  });

  it("queues terminal commands per terminal and consumes only matching ones", () => {
    const first = useTaskStore.getState().queueTerminalCommand("build", "npm run build", "t1");
    useTaskStore.getState().queueTerminalCommand("test", "npm test", "t2");

    expect(useTaskStore.getState().lastTask).toMatchObject({ terminalId: "t2" });
    expect(useTaskStore.getState().pendingTerminalCommands).toHaveLength(2);

    const consumed = useTaskStore.getState().consumeTerminalCommands("t1");
    expect(consumed.map((entry) => entry.id)).toEqual([first.id]);
    expect(useTaskStore.getState().pendingTerminalCommands).toHaveLength(1);
    expect(useTaskStore.getState().consumeTerminalCommands("t1")).toEqual([]);
  });

  it("queues a terminal session request together with a started run", () => {
    const request = useTaskStore
      .getState()
      .queueTerminalSession("run", "Run", "npm run tauri -- dev", "D:/work/agent-ide");

    expect(request.taskId).toBe("run");
    expect(request.terminalId).toMatch(/^task-terminal-/);
    expect(request.exitMarker).toContain(request.id);
    expect(request.cwd).toBe("D:/work/agent-ide");

    const state = useTaskStore.getState();
    expect(state.pendingTerminalSessionRequests).toHaveLength(1);
    expect(state.taskRuns.run).toMatchObject({ runId: request.runId, status: "running" });

    const consumed = useTaskStore.getState().consumeTerminalSessionRequests();
    expect(consumed.map((entry) => entry.id)).toEqual([request.id]);
    expect(useTaskStore.getState().consumeTerminalSessionRequests()).toEqual([]);
  });

  it("appends terminal output with a bounded buffer and clears selectively", () => {
    useTaskStore.getState().appendTerminalOutput("t1", "a".repeat(12000));
    useTaskStore.getState().appendTerminalOutput("t1", "tail");

    const output = useTaskStore.getState().terminalOutput.t1;
    expect(output).toHaveLength(12000);
    expect(output.endsWith("tail")).toBe(true);

    useTaskStore.getState().appendTerminalOutput("t2", "other");
    useTaskStore.getState().clearTerminalOutput("t1");

    expect(useTaskStore.getState().terminalOutput.t1).toBeUndefined();
    expect(useTaskStore.getState().terminalOutput.t2).toBe("other");

    useTaskStore.getState().clearTerminalOutput();
    expect(useTaskStore.getState().terminalOutput).toEqual({});
  });
});
