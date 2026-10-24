import type { AgentRole } from "../../types/agent";
import type { MessageKey } from "../../i18n/messages";

/**
 * 五个角色只在这里列一次。
 *
 * 名字和说明以前在两个地方各写一遍（`AgentSelector` 的卡片、`PipelineEditor` 的下拉），
 * 于是同一个角色可以有两种叫法，而且翻译时只改一处就会半中半英。图标留在代码里 ——
 * 它不是文案，两种语言都一样。
 */
export const AGENT_ROLES: readonly { id: AgentRole; icon: string }[] = [
  { id: "architect", icon: "🏗" },
  { id: "designer", icon: "📐" },
  { id: "coder", icon: "💻" },
  { id: "tester", icon: "🧪" },
  { id: "reviewer", icon: "🔍" },
] as const;

export function roleLabelKey(role: AgentRole): MessageKey {
  return `role.${role}`;
}

export function roleDescKey(role: AgentRole): MessageKey {
  return `role.${role}.desc`;
}

/** 下拉框里按角色取图标。没有的角色返回空串，不编一个别的图标出来 */
export function roleIcon(role: AgentRole): string {
  return AGENT_ROLES.find((item) => item.id === role)?.icon ?? "";
}
