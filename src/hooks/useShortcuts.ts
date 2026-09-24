import { useEffect, useRef } from "react";
import { useLayoutStore } from "../stores/useLayoutStore";
import { useEditorStore } from "../stores/useEditorStore";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { isTauriRuntime } from "../utils/tauri";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n/messages";

export interface Shortcut {
  id: string;
  keys: string;          // e.g. "Ctrl+S"
  /** 界面上显示的名字走文案键：这一串以前是英文写死的，帮助弹窗是唯一的消费者 */
  labelKey: MessageKey;
  /** 分组用 id，不用显示名 —— 显示名由 `shortcut.group.*` 给，帮助弹窗以前另存了一份映射表 */
  group: "panels" | "git" | "navigation" | "editor" | "general";
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
  // 只有"打开文件夹"那个系统对话框的标题需要现译：其它名字是给帮助弹窗看的键
  const t = useT();

  /** 定义所有全局快捷键 */
  const shortcuts: Shortcut[] = [
    // Panels
    { id: "command-palette", keys: "Ctrl+Shift+P", labelKey: "shortcut.commandPalette", group: "general", scope: "global",
      handler: () => window.dispatchEvent(new CustomEvent("toggle-command-palette")) },
    { id: "toggle-explorer", keys: "Ctrl+Shift+E", labelKey: "shortcut.toggleExplorer", group: "panels", scope: "global",
      handler: () => toggleLeftPanel() },
    { id: "toggle-agent", keys: "Ctrl+Shift+X", labelKey: "shortcut.toggleAgent", group: "panels", scope: "global",
      handler: () => toggleRightPanel() },
    { id: "toggle-terminal", keys: "Ctrl+`", labelKey: "shortcut.toggleTerminal", group: "panels", scope: "global",
      handler: () => toggleBottomPanel() },
    { id: "toggle-focus", keys: "Ctrl+Shift+F", labelKey: "shortcut.toggleFocus", group: "panels", scope: "global",
      handler: () => toggleFocusMode() },

    // Git
    { id: "git-panel", keys: "Ctrl+Shift+G", labelKey: "shortcut.gitPanel", group: "git", scope: "global",
      handler: () => { setLeftTab("git"); useLayoutStore.getState().leftVisible || toggleLeftPanel(); } },

    // Navigation
    { id: "explorer-panel", keys: "Ctrl+Shift+D", labelKey: "shortcut.explorerPanel", group: "navigation", scope: "global",
      handler: () => { setLeftTab("explorer"); useLayoutStore.getState().leftVisible || toggleLeftPanel(); } },
    { id: "terminal-bottom", keys: "Ctrl+Shift+T", labelKey: "shortcut.terminalBottom", group: "navigation", scope: "global",
      handler: () => { setBottomTab("terminal"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },
    { id: "commands-bottom", keys: "Ctrl+Shift+B", labelKey: "shortcut.commandsBottom", group: "navigation", scope: "global",
      handler: () => { setBottomTab("commands"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },
    { id: "logs-bottom", keys: "Ctrl+Shift+L", labelKey: "shortcut.logsBottom", group: "navigation", scope: "global",
      handler: () => { setBottomTab("logs"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },
    { id: "problems-bottom", keys: "Ctrl+Shift+M", labelKey: "shortcut.problemsBottom", group: "navigation", scope: "global",
      handler: () => { setBottomTab("problems"); useLayoutStore.getState().bottomVisible || toggleBottomPanel(); } },

    // File
    { id: "open-folder", keys: "Ctrl+O", labelKey: "shortcut.openFolder", group: "general", scope: "global",
      handler: async () => {
        try {
          if (!isTauriRuntime()) return;
          const selected = await open({ directory: true, multiple: false, title: t("shortcut.openFolder.dialog") });
          if (selected && typeof selected === "string") {
            await invoke("save_workspace_path", { path: selected });
            useLayoutStore.getState().setWorkspacePath(selected);
            useEditorStore.getState().setWorkspacePath(selected);
          }
        } catch { /* ignore */ }
      } },
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
