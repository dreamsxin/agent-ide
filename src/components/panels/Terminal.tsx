import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { isTauriRuntime } from "../../utils/tauri";
import { appendAndParseTerminalProblems } from "../../utils/terminalProblemParser";
import { useProblemStore } from "../../stores/useProblemStore";
import { useTaskStore } from "../../stores/useTaskStore";
import { useLogStore } from "../../stores/useLogStore";
import { useLayoutStore } from "../../stores/useLayoutStore";

interface TerminalProps {
  terminalId: string;
  cwd: string;
  profile: string;
  initialCommand?: string;
  taskId?: string;
  taskLabel?: string;
  taskRunId?: string;
  taskExitMarker?: string;
}

interface TerminalSession {
  id: string;
  label: string;
  cwd: string;
  profile: string;
  version: number;
  initialCommand?: string;
  taskId?: string;
  taskLabel?: string;
  taskRunId?: string;
  taskExitMarker?: string;
}

export default function TerminalPanel() {
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const pendingSessionRequestCount = useTaskStore((s) => s.pendingTerminalSessionRequests.length);
  const consumeTerminalSessionRequests = useTaskStore((s) => s.consumeTerminalSessionRequests);
  const [sessions, setSessions] = useState<TerminalSession[]>(() => [
    createTerminalSession("main", "1", workspacePath),
  ]);
  const [activeId, setActiveId] = useState("main");

  const activeSession = sessions.find((session) => session.id === activeId) ?? sessions[0];

  useEffect(() => {
    if (!workspacePath) return;
    setSessions((current) =>
      current.map((session) =>
        session.id === "main" && session.cwd !== workspacePath
          ? { ...session, cwd: workspacePath, version: session.version + 1 }
          : session
      )
    );
  }, [workspacePath]);

  useEffect(() => {
    if (pendingSessionRequestCount === 0) return;
    const requests = consumeTerminalSessionRequests();
    if (requests.length === 0) return;
    setSessions((current) => {
      const next = [...current];
      for (const request of requests) {
        next.push(
          createTerminalSession(
            request.terminalId,
            request.label,
            request.cwd || workspacePath,
            request.command,
            {
              taskId: request.taskId,
              taskLabel: request.label,
              taskRunId: request.runId,
              taskExitMarker: request.exitMarker,
            }
          )
        );
      }
      return next;
    });
    setActiveId(requests[requests.length - 1].terminalId);
  }, [consumeTerminalSessionRequests, pendingSessionRequestCount, workspacePath]);

  const createSession = useCallback(() => {
    setSessions((current) => {
      const nextIndex = current.length + 1;
      const session = createTerminalSession(`terminal-${Date.now()}`, String(nextIndex), workspacePath);
      setActiveId(session.id);
      return [...current, session];
    });
  }, [workspacePath]);

  const restartSession = useCallback(async (sessionId: string) => {
    await invoke("kill_terminal", { id: sessionId }).catch(() => undefined);
    setSessions((current) =>
      current.map((session) =>
        session.id === sessionId
          ? { ...session, cwd: workspacePath || session.cwd, version: session.version + 1 }
          : session
      )
    );
  }, [workspacePath]);

  const closeSession = useCallback((sessionId: string) => {
    setSessions((current) => {
      if (current.length <= 1) {
        void restartSession(sessionId);
        return current;
      }
      const index = current.findIndex((session) => session.id === sessionId);
      const next = current.filter((session) => session.id !== sessionId);
      if (activeId === sessionId) {
        const fallback = next[Math.max(0, index - 1)] ?? next[0];
        setActiveId(fallback.id);
      }
      return next;
    });
  }, [activeId, restartSession]);

  return (
    <div className="flex h-full flex-col bg-black">
      <div className="flex items-center gap-1 border-b border-surface-border bg-surface-panel px-2 py-1">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-auto">
          {sessions.map((session) => (
            <button
              key={session.id}
              onClick={() => setActiveId(session.id)}
              className={`flex max-w-[180px] items-center gap-1 rounded border px-2 py-0.5 text-[11px] ${
                activeId === session.id
                  ? "border-accent-blue/50 bg-accent-blue/10 text-surface-text"
                  : "border-surface-border text-surface-muted hover:text-surface-text"
              }`}
              title={`${session.profile} - ${session.cwd || "workspace"}`}
            >
              <span>{">"}</span>
              <span className="truncate">Terminal {session.label}</span>
            </button>
          ))}
        </div>
        <span className="hidden max-w-[40%] truncate font-mono text-[10px] text-surface-muted md:inline">
          {activeSession?.profile} · {activeSession?.cwd || "No workspace"}
        </span>
        {activeSession && (
          <>
            <button
              onClick={() => void restartSession(activeSession.id)}
              className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text"
              title="Restart terminal"
            >
              Restart
            </button>
            <button
              onClick={createSession}
              className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text"
              title="New terminal"
            >
              +
            </button>
            <button
              onClick={() => closeSession(activeSession.id)}
              className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-diff-remove"
              title="Close terminal"
            >
              x
            </button>
          </>
        )}
      </div>

      <div className="min-h-0 flex-1">
        {sessions.map((session) => (
          <div
            key={`${session.id}-${session.version}`}
            className={activeId === session.id ? "h-full" : "hidden h-full"}
          >
            <TerminalSessionView
              terminalId={session.id}
              cwd={session.cwd}
              profile={session.profile}
              initialCommand={session.initialCommand}
              taskId={session.taskId}
              taskLabel={session.taskLabel}
              taskRunId={session.taskRunId}
              taskExitMarker={session.taskExitMarker}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

function TerminalSessionView({
  terminalId,
  cwd,
  profile,
  initialCommand,
  taskId,
  taskLabel,
  taskRunId,
  taskExitMarker,
}: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const readyRef = useRef(false);
  const initialCommandRef = useRef(initialCommand);
  const outputBufferRef = useRef("");
  const taskOutputRef = useRef("");
  const taskCompletedRef = useRef(false);
  const replaceProblems = useProblemStore((s) => s.replaceProblems);
  const lastTask = useTaskStore((s) => s.lastTask);
  const consumeTerminalCommands = useTaskStore((s) => s.consumeTerminalCommands);
  const appendTerminalOutput = useTaskStore((s) => s.appendTerminalOutput);
  const clearTerminalOutput = useTaskStore((s) => s.clearTerminalOutput);
  const finishTaskRun = useTaskStore((s) => s.finishTaskRun);
  const addLog = useLogStore((s) => s.addLog);
  const [startupError, setStartupError] = useState<string | null>(null);

  const runQueuedCommands = useCallback(() => {
    if (!readyRef.current) return;
    const term = xtermRef.current;
    const commands = consumeTerminalCommands(terminalId);
    for (const queued of commands) {
      const data = `${queued.command}\r`;
      term?.writeln(`\r\n\x1b[90m$ ${queued.command}\x1b[0m`);
      invoke("write_to_terminal", { id: terminalId, data }).catch((err) => {
        console.warn("[Terminal] queued command failed:", err);
        addLog({
          time: new Date().toLocaleTimeString(),
          level: "error",
          source: "system",
          message: `Failed to run project task: ${queued.command}`,
          details: String(err),
        });
      });
    }
  }, [addLog, consumeTerminalCommands, terminalId]);

  const runInitialCommand = useCallback(() => {
    if (!readyRef.current) return;
    const command = initialCommandRef.current;
    if (!command) return;
    initialCommandRef.current = undefined;
    const data = taskExitMarker ? `${buildTrackedCommand(command, taskExitMarker)}\r` : `${command}\r`;
    xtermRef.current?.writeln(`\r\n\x1b[90m$ ${command}\x1b[0m`);
    invoke("write_to_terminal", { id: terminalId, data }).catch((err) => {
      console.warn("[Terminal] initial command failed:", err);
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "error",
        source: "system",
        message: `Failed to run project task: ${command}`,
        details: String(err),
      });
    });
  }, [addLog, taskExitMarker, terminalId]);

  const handleTaskOutput = useCallback(
    (chunk: string) => {
      if (!taskId || !taskExitMarker || taskCompletedRef.current) return;
      taskOutputRef.current = `${taskOutputRef.current}${chunk}`;
      const plainOutput = stripAnsi(taskOutputRef.current).replace(/\r\n/g, "\n");
      const markerIndex = plainOutput.indexOf(taskExitMarker);
      if (markerIndex < 0) return;

      const beforeMarker = plainOutput.slice(0, markerIndex);
      const markerTail = plainOutput.slice(markerIndex);
      const exitMatch = markerTail.match(new RegExp(`${escapeRegExp(taskExitMarker)}:(-?\\d+)`));
      if (!exitMatch) return;
      const exitCode = exitMatch ? Number(exitMatch[1]) : null;
      const status = exitCode === 0 ? "success" : "failed";
      const output = cleanupTrackedOutput(beforeMarker, initialCommand ?? "");
      const problems = appendAndParseTerminalProblems("", output, terminalId).problems;

      taskCompletedRef.current = true;
      finishTaskRun(taskId, status, {
        exitCode,
        durationMs: Date.now() - Number(taskRunId?.split("-")[1] ?? Date.now()),
        output,
      });
      replaceProblems("test", problems);
      addLog({
        time: new Date().toLocaleTimeString(),
        level: status === "success" ? "success" : "error",
        source: "system",
        message: `${taskLabel ?? taskId} ${status}${exitCode === null ? "" : ` (exit ${exitCode})`}`,
        details: output.slice(-4000),
      });
    },
    [
      addLog,
      finishTaskRun,
      initialCommand,
      replaceProblems,
      taskExitMarker,
      taskId,
      taskLabel,
      taskRunId,
      terminalId,
    ]
  );

  useEffect(() => {
    if (!isTauriRuntime()) return;
    if (!containerRef.current) return;
    setStartupError(null);
    outputBufferRef.current = "";
    clearTerminalOutput(terminalId);
    replaceProblems("test", []);

    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    const term = new XtermTerminal({
      cursorBlink: true,
      cursorStyle: "bar",
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Consolas', monospace",
      theme: {
        background: "#0D1117",
        foreground: "#C9D1D9",
        cursor: "#3B82F6",
        selectionBackground: "#3B82F644",
        black: "#484F58",
        red: "#DA3633",
        green: "#238636",
        yellow: "#D29922",
        blue: "#58A6FF",
        magenta: "#BC8CFF",
        cyan: "#39C5CF",
        white: "#B1BAC4",
        brightBlack: "#6E7681",
        brightRed: "#FF7B72",
        brightGreen: "#3FB950",
        brightYellow: "#E3B341",
        brightBlue: "#79C0FF",
        brightMagenta: "#D2A8FF",
        brightCyan: "#56D4DD",
        brightWhite: "#F0F6FC",
      },
      allowProposedApi: true,
      scrollback: 5000,
      cols: 80,
      rows: 24,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);
    try {
      term.open(containerRef.current);
      term.writeln(`\x1b[90mStarting ${profile} in ${cwd || "workspace"}...\x1b[0m`);
      requestAnimationFrame(() => {
        try {
          fitAddon.fit();
        } catch {
          // The panel may still be measuring during startup.
        }
      });
    } catch {
      term.dispose();
      return;
    }

    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    const sendResize = () => {
      if (!readyRef.current || disposed) return;
      const dims = fitAddonRef.current?.proposeDimensions();
      if (!dims) return;
      invoke("resize_terminal", {
        id: terminalId,
        cols: dims.cols,
        rows: dims.rows,
      }).catch((err) => console.warn("[Terminal] resize failed:", err));
    };

    listen<{ id: string; data: string }>("terminal-output", (event) => {
      if (event.payload.id === terminalId) {
        term.write(event.payload.data);
        appendTerminalOutput(terminalId, event.payload.data);
        handleTaskOutput(event.payload.data);
        const parsed = appendAndParseTerminalProblems(
          outputBufferRef.current,
          event.payload.data,
          terminalId
        );
        outputBufferRef.current = parsed.buffer;
        replaceProblems("test", parsed.problems);
      }
    })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((err) => console.warn("[Terminal] listen failed:", err));

    invoke("spawn_terminal", { id: terminalId, cwd: cwd || null })
      .then(() => {
        readyRef.current = true;
        sendResize();
        runInitialCommand();
        runQueuedCommands();
      })
      .catch((err) => {
        const msg = String(err);
        if (msg.includes("already exists")) {
          readyRef.current = true;
          sendResize();
          runInitialCommand();
          runQueuedCommands();
          return;
        }
        term.writeln(`\r\n\x1b[31mTerminal failed to start: ${msg}\x1b[0m`);
        setStartupError(msg);
      });

    term.onData((data) => {
      if (!readyRef.current) return;
      invoke("write_to_terminal", { id: terminalId, data }).catch((err) =>
        console.warn("[Terminal] write failed:", err)
      );
    });

    runQueuedCommands();

    const resizeObserver = new ResizeObserver(() => {
      if (fitAddonRef.current) {
        try {
          const rect = containerRef.current?.getBoundingClientRect();
          if (!rect || rect.width <= 0 || rect.height <= 0) return;
          fitAddonRef.current.fit();
          sendResize();
        } catch {
          // Hidden panels can briefly report invalid dimensions.
        }
      }
    });

    resizeObserver.observe(containerRef.current);

    return () => {
      disposed = true;
      readyRef.current = false;
      outputBufferRef.current = "";
      resizeObserver.disconnect();
      unlisten?.();
      invoke("kill_terminal", { id: terminalId }).catch((err) =>
        console.warn("[Terminal] kill failed:", err)
      );
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
    };
  }, [
    appendTerminalOutput,
    clearTerminalOutput,
    replaceProblems,
    runQueuedCommands,
    runInitialCommand,
    handleTaskOutput,
    terminalId,
    cwd,
    profile,
  ]);

  useEffect(() => {
    runQueuedCommands();
  }, [lastTask, runQueuedCommands]);

  return (
    <div
      ref={containerRef}
      className="h-full w-full min-h-[120px] relative"
      style={{ padding: "4px 8px" }}
    >
      {!isTauriRuntime() && (
        <div className="h-full flex items-center justify-center text-xs text-surface-muted">
          Terminal is available in the Tauri app runtime.
        </div>
      )}
      {isTauriRuntime() && startupError && (
        <div className="absolute right-2 top-2 max-w-[60%] rounded border border-diff-remove/40 bg-surface-panel px-2 py-1 text-[10px] text-diff-remove">
          {startupError}
        </div>
      )}
    </div>
  );
}

function createTerminalSession(
  id: string,
  label: string,
  cwd: string,
  initialCommand?: string,
  taskMeta?: Pick<TerminalSession, "taskId" | "taskLabel" | "taskRunId" | "taskExitMarker">
): TerminalSession {
  return {
    id,
    label,
    cwd,
    profile: navigator.userAgent.includes("Windows") ? "cmd.exe" : "system shell",
    version: 0,
    initialCommand,
    ...taskMeta,
  };
}

function buildTrackedCommand(command: string, marker: string) {
  if (navigator.userAgent.includes("Windows")) {
    return `${command} & call echo ${marker}:%%ERRORLEVEL%%`;
  }
  return `${command}; printf '\\n${marker}:%s\\n' "$?"`;
}

function stripAnsi(value: string) {
  return value.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function cleanupTrackedOutput(output: string, command: string) {
  return output
    .replace(command, "")
    .split("\n")
    .filter((line) => !line.includes("__AGENT_IDE_TASK_EXIT_"))
    .join("\n")
    .trim();
}
