/**
 * Shared Agent action definitions and prompt builders.
 * Used by QuickActions floating toolbar, editor context menu, and Monaco CodeAction lightbulb.
 */

import type { MessageKey } from "../i18n/messages";
import { translate, useLocaleStore } from "../i18n";

export interface AgentQuickAction {
  key: string;
  /** 界面上的名字走文案键；`prompt` 不走，它是发给模型的，语言由模型那边决定 */
  labelKey: MessageKey;
  icon: string;
  prompt: string;
}

/** Registered Agent quick actions. */
export const AGENT_QUICK_ACTIONS: readonly AgentQuickAction[] = [
  {
    key: "explain",
    labelKey: "action.explain",
    icon: "\u{1F4A1}",
    prompt:
      "Explain the selected code. Focus on behavior, inputs, outputs, side effects, and any hidden assumptions.",
  },
  {
    key: "fix",
    labelKey: "action.fix",
    icon: "\u{1F527}",
    prompt:
      "Find and fix bugs in the selected code. Return proposed code changes as reviewable diffs when a code change is needed.",
  },
  {
    key: "refactor",
    labelKey: "action.refactor",
    icon: "\u{267B}\u{FE0F}",
    prompt:
      "Refactor the selected code for readability and maintainability without changing behavior. Return proposed code changes as reviewable diffs.",
  },
  {
    key: "optimize",
    labelKey: "action.optimize",
    icon: "\u{26A1}",
    prompt:
      "Optimize the selected code only where there is a clear performance or complexity benefit. Explain the tradeoff and return reviewable diffs if changing code.",
  },
] as const;

/**
 * 「图标 + 名字 + with Agent」这一串在三处出现：右键菜单、灯泡标题、灯泡命令。
 * 之前三处各拼一次，中文的语序又和英文不同（「让 Agent 修一下」），所以拼接只留一份。
 *
 * 不收 `t`，因为调用点里有一个在模块层（Monaco 的注册），拿不到 React hook。
 * 现读当前语言：菜单和灯泡都是被点的那一刻才取标题的。
 */
export function agentActionLabel(action: AgentQuickAction, withIcon = true): string {
  const locale = useLocaleStore.getState().locale;
  const label = translate(locale, "action.withAgent", {
    label: translate(locale, action.labelKey),
  });
  return withIcon ? `${action.icon} ${label}` : label;
}

export type AgentQuickActionKey = (typeof AGENT_QUICK_ACTIONS)[number]["key"];

/** Build a full Agent prompt from a quick action and selection info. */
export function buildActionPrompt(
  action: AgentQuickActionKey,
  selectedText: string,
  activeFile: string | null,
  startLine?: number,
  endLine?: number
): string {
  const rangeText = startLine != null
    ? `lines ${startLine}-${endLine ?? startLine}`
    : "current selection";
  const fileText = activeFile ? ` in ${activeFile}` : "";

  const actionDef = AGENT_QUICK_ACTIONS.find((a) => a.key === action);
  const instruction = actionDef?.prompt ?? action;

  return `${instruction}

Target: ${rangeText}${fileText}

Selected code:
\`\`\`
${selectedText}
\`\`\``;
}

/** Monaco-specific action IDs used for context menu registration. */
export const AGENT_ACTION_IDS = {
  explain: "agent-ide.explain-selection",
  fix: "agent-ide.fix-selection",
  refactor: "agent-ide.refactor-selection",
  optimize: "agent-ide.optimize-selection",
} as const;
