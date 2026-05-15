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
  terminalId?: string;
}

export default function Terminal({ terminalId = "main" }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const readyRef = useRef(false);
  const outputBufferRef = useRef("");
  const replaceProblems = useProblemStore((s) => s.replaceProblems);
  const lastTask = useTaskStore((s) => s.lastTask);
  const consumeTerminalCommands = useTaskStore((s) => s.consumeTerminalCommands);
  const appendTerminalOutput = useTaskStore((s) => s.appendTerminalOutput);
  const clearTerminalOutput = useTaskStore((s) => s.clearTerminalOutput);
  const addLog = useLogStore((s) => s.addLog);
  const workspacePath = useLayoutStore((s) => s.workspacePath);
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
      term.writeln("\x1b[90mStarting terminal...\x1b[0m");
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

    invoke("spawn_terminal", { id: terminalId, cwd: workspacePath || null })
      .then(() => {
        readyRef.current = true;
        sendResize();
        runQueuedCommands();
      })
      .catch((err) => {
        const msg = String(err);
        if (msg.includes("already exists")) {
          readyRef.current = true;
          sendResize();
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
    terminalId,
    workspacePath,
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
