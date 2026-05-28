/**
 * Shared Agent action definitions and prompt builders.
 * Used by QuickActions floating toolbar, editor context menu, and Monaco CodeAction lightbulb.
 */

export interface AgentQuickAction {
  key: string;
  label: string;
  icon: string;
  prompt: string;
}

/** Registered Agent quick actions. */
export const AGENT_QUICK_ACTIONS: readonly AgentQuickAction[] = [
  {
    key: "explain",
    label: "Explain",
    icon: "\u{1F4A1}",
    prompt:
      "Explain the selected code. Focus on behavior, inputs, outputs, side effects, and any hidden assumptions.",
  },
  {
    key: "fix",
    label: "Fix",
    icon: "\u{1F527}",
    prompt:
      "Find and fix bugs in the selected code. Return proposed code changes as reviewable diffs when a code change is needed.",
  },
  {
    key: "refactor",
    label: "Refactor",
    icon: "\u{267B}\u{FE0F}",
    prompt:
      "Refactor the selected code for readability and maintainability without changing behavior. Return proposed code changes as reviewable diffs.",
  },
  {
    key: "optimize",
    label: "Optimize",
    icon: "\u{26A1}",
    prompt:
      "Optimize the selected code only where there is a clear performance or complexity benefit. Explain the tradeoff and return reviewable diffs if changing code.",
  },
] as const;

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
