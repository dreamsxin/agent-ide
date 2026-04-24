import { useEffect, useRef } from "react";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

interface TerminalProps {
  /** 终端 ID（用于后续多终端支持） */
  terminalId?: string;
}

export default function Terminal({ terminalId = "main" }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // 初始化 xterm 实例
  useEffect(() => {
    if (!containerRef.current) return;

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
    term.open(containerRef.current);
    fitAddon.fit();

    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // 用户输入 → 发送到终端
    term.onData((data) => {
      if (xtermRef.current) {
        xtermRef.current.write(data);
      }
    });

    // 欢迎信息
    term.writeln("  \x1b[36m🧠 Agent IDE Terminal\x1b[0m");
    term.writeln("  ─────────────────────────────");
    term.writeln("");

    // 监听 resize
    const resizeObserver = new ResizeObserver(() => {
      if (fitAddonRef.current) {
        try {
          fitAddonRef.current.fit();
        } catch {
          // fit 可能在不可见时失败，忽略
        }
      }
    });

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      resizeObserver.disconnect();
      term.dispose();
    };
  }, [terminalId]);

  return (
    <div
      ref={containerRef}
      className="h-full w-full"
      style={{ padding: "4px 8px" }}
    />
  );
}
