import { useEffect, useRef } from "react";
import { useLayoutStore } from "../stores/useLayoutStore";

export interface Shortcut {
  id: string;
  keys: string;          // e.g. "Ctrl+S"
  label: string;
  group: string;         // "Editor" | "Panels" | "Navigation" | "Git" | "General"
  scope: "global" | "editor";
  handler: () => void;
}

/** 解析按键字符串为匹配函数 */
function matchKeys(combo: string, e: KeyboardEvent): boolean {
  const parts = combo.toLowerCase().split("+");
  const hasCtrl = parts.includes("ctrl") || parts.includes("cmd");
  const hasShift = parts.includes("shift");
  const hasAlt = parts.includes("alt");

  const ctrlOk = hasCtrl === (e.ctrlKey || e.metaKey);
  const shiftOk = hasShift === e.shiftKey;
  const altOk = hasAlt === e.altKey;

  if (!ctrlOk || !shiftOk || !altOk) return false;

  const keyPart = parts.find(
    (p) => !["ctrl", "cmd", "shift", "alt"].includes(p)
  );
  if (!keyPart) return false;

  // 特殊键名映射
  const keyMap: Record<string, string> = {
    "`": "`",
    escape: "escape",
    enter: "enter",
    tab: "tab",
    space: " ",
    f1: "f1", f2: "f2", f3: "f3", f4: "f4",
    f5: "f5", f6: "f6", f7: "f7", f8: "f8",
    f9: "f9", f10: "f10", f11: "f11", f12: "f12",
    up: "arrowup", down: "arrowdown",
    left: "arrowleft", right: "arrowright",
  };

  const expectedKey = keyMap[keyPart] ?? keyPart;
  return e.key.toLowerCase() === expectedKey;
}

export default function useShortcuts() {
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const toggleFocusMode = useLayoutStore((s) => s.toggleFocusMode);
  const setLeftTab = useLayoutStore((s) => s.setLeftTab);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);

  /** 定义所有全局快捷键 */
  const shortcuts: Shortcut[] = [
    // Panels
    { id: "toggle-explorer", keys: "Ctrl+Shift+E", label: "Toggle Explorer", group: "Panels", scope: "global",
      handler: () => toggleLeftPanel() },
    { id: "toggle-agent", keys: "Ctrl+Shift+X", label: "Toggle Agent Panel", group: "Panels", scope: "global",
      handler: () => toggleRightPanel() },
    { id: "toggle-terminal", keys: "Ctrl+`", label: "Toggle Terminal", group: "Panels", scope: "global",
      handler: () => toggleBottomPanel() },
    { id: "toggle-focus", keys: "Ctrl+Shift+F", label: "Toggle Focus Mode", group: "Panels", scope: "global",
      handler: () => toggleFocusMode() },

    // Git
    { id: "git-panel", keys: "Ctrl+Shift+G", label: "Git Panel", group: "Git", scope: "global",
      handler: () => { setLeftTab("git"); useLayoutStore.getState().leftVisible || toggleLeftPanel(); } },

    // Navigation
    { id: "explorer-panel", keys: "Ctrl+Shift+D", label: "Explorer Panel", group: "Navigation", scope: "global",
      handler: () => { setLeftTab("explorer"); useLayoutStore.getState().leftVisible || toggleLeftPanel(); } },
    { id: "terminal-bottom", keys: "Ctrl+Shift+T", label: "Switch to Terminal", group: "Navigation", scope: "global",
      handler: () => { setBottomTab("terminal"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },
    { id: "logs-bottom", keys: "Ctrl+Shift+L", label: "Switch to Logs", group: "Navigation", scope: "global",
      handler: () => { setBottomTab("logs"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },
  ];

  const shortcutsRef = useRef(shortcuts);
  shortcutsRef.current = shortcuts;

  // 全局快捷键监听
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // 跳过输入区域内的非组合键
      const target = e.target as HTMLElement;
      const inEditor =
        target.closest(".monaco-editor") ||
        target.closest('[role="code"]') ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "INPUT";

      if (inEditor && !e.ctrlKey && !e.metaKey) return;

      for (const shortcut of shortcutsRef.current) {
        if (matchKeys(shortcut.keys, e)) {
          e.preventDefault();
          e.stopPropagation();
          shortcut.handler();
          return;
        }
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  return { shortcuts };
}
