import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { isTauriRuntime } from "../../utils/tauri";

interface TerminalProps {
  terminalId?: string;
}

export default function Terminal({ terminalId = "main" }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const readyRef = useRef(false);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    if (!containerRef.current) return;

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

    invoke("spawn_terminal", { id: terminalId })
      .then(() => {
        readyRef.current = true;
        sendResize();
      })
      .catch((err) => {
        const msg = String(err);
        if (msg.includes("already exists")) {
          readyRef.current = true;
          sendResize();
          return;
        }
        term.writeln(`\r\n\x1b[31mTerminal failed to start: ${msg}\x1b[0m`);
      });

    term.onData((data) => {
      if (!readyRef.current) return;
      invoke("write_to_terminal", { id: terminalId, data }).catch((err) =>
        console.warn("[Terminal] write failed:", err)
      );
    });

    const resizeObserver = new ResizeObserver(() => {
      if (fitAddonRef.current) {
        try {
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
      resizeObserver.disconnect();
      unlisten?.();
      invoke("kill_terminal", { id: terminalId }).catch((err) =>
        console.warn("[Terminal] kill failed:", err)
      );
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
    };
  }, [terminalId]);

  return (
    <div
      ref={containerRef}
      className="h-full w-full"
      style={{ padding: "4px 8px" }}
    >
      {!isTauriRuntime() && (
        <div className="h-full flex items-center justify-center text-xs text-surface-muted">
          Terminal is available in the Tauri app runtime.
        </div>
      )}
    </div>
  );
}
