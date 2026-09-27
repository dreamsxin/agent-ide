import type { AgentState } from "../../types/agent";
import { agentStateMessageKey } from "../../utils/agentExperience";
import { useT } from "../../i18n";

/**
 * 每档状态的点色和是否呼吸。
 *
 * 文字不在这里：八个状态的标签原本是写死的英文，而同一批状态在对话面板里是走 i18n 的
 * （`agentStateMessageKey`）—— 于是切到中文时，底部状态栏还在说 "Waiting"。
 * 一份状态两套说法，哪一套都不该由一个装饰性组件自己决定。
 */
const stateConfig: Record<AgentState, { color: string; animate: boolean }> = {
  idle: { color: "bg-gray-500", animate: false },
  thinking: { color: "bg-purple-500", animate: true },
  planning: { color: "bg-yellow-500", animate: true },
  acting: { color: "bg-blue-500", animate: true },
  reviewing: { color: "bg-orange-500", animate: true },
  waiting_user: { color: "bg-cyan-500", animate: true },
  done: { color: "bg-green-500", animate: false },
  error: { color: "bg-red-500", animate: false },
};

interface StatusDotProps {
  state: AgentState;
  showLabel?: boolean;
}

export default function StatusDot({ state, showLabel = true }: StatusDotProps) {
  const config = stateConfig[state] ?? stateConfig.idle;
  const t = useT();

  return (
    <div className="flex items-center gap-1.5">
      <span
        className={`inline-block w-2 h-2 rounded-full ${config.color} ${
          config.animate ? "animate-pulse-dot" : ""
        }`}
      />
      {showLabel && (
        <span className="text-xs text-surface-muted">{t(agentStateMessageKey(state))}</span>
      )}
    </div>
  );
}
