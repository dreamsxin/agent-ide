import { useState } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { AgentRole } from "../../types/agent";
import { isAgentBusy } from "../../utils/agentExperience";
import { useT } from "../../i18n";
import { AGENT_ROLES, roleDescKey, roleLabelKey } from "./agentRoles";
import PipelineEditor from "./PipelineEditor";

export default function AgentSelector() {
  const t = useT();
  const mode = useAgentStore((s) => s.mode);
  const ideMode = useAgentStore((s) => s.ideMode);
  const state = useAgentStore((s) => s.state);
  const activeRole = useAgentStore((s) => s.activeRole);
  const setActiveRole = useAgentStore((s) => s.setActiveRole);
  const pipeline = useAgentStore((s) => s.pipeline);
  // 同 TopBar：`waiting_user` 是"它在等你"，那时候换角色是合理的
  const isRunning = isAgentBusy(state);

  const [showEditor, setShowEditor] = useState(false);

  const handleRoleClick = (role: AgentRole) => {
    if (isRunning) return;
    setActiveRole(role);
  };

  return (
    <div className="p-3 text-xs">
      <div className="text-surface-muted mb-2 font-semibold tracking-wide flex items-center justify-between">
        <span>{t("selector.title")}</span>
        {isRunning && (
          <span className="text-[10px] text-accent-blue animate-pulse">{t("selector.busy")}</span>
        )}
      </div>

      {/* 角色卡片 */}
      <div className="space-y-1.5">
        {AGENT_ROLES.map((role) => {
          const isActive = activeRole === role.id;
          return (
            <div
              key={role.id}
              onClick={() => handleRoleClick(role.id)}
              className={`flex items-start gap-2 p-2 rounded border transition-colors ${
                isRunning
                  ? "border-surface-border/30 opacity-50 cursor-not-allowed"
                  : isActive
                  ? "border-accent-blue bg-accent-blue/10 cursor-pointer"
                  : "border-surface-border cursor-pointer hover:border-accent-blue/40 hover:bg-surface-border/10"
              }`}
            >
              <span className="text-sm mt-0.5 flex-shrink-0">{role.icon}</span>
              <div className="min-w-0">
                <div className="font-medium text-surface-text">
                  {t(roleLabelKey(role.id))}
                  {isActive && (
                    <span className="ml-1.5 text-[10px] text-accent-blue font-normal">
                      {t("selector.active")}
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-surface-muted leading-tight">
                  {t(roleDescKey(role.id))}
                </div>
              </div>
              <div className="ml-auto flex-shrink-0">
                <span
                  className={`inline-block w-2 h-2 rounded-full ${
                    isActive ? "bg-accent-blue" : "bg-accent-green"
                  }`}
                />
              </div>
            </div>
          );
        })}
      </div>

      {/* 流水线信息 + 编辑入口 */}
      <div className="mt-3 pt-2 border-t border-surface-border">
        <div className="flex items-center justify-between">
          <div className="text-surface-muted text-[10px]">
            {t("selector.pipeline", { stages: pipeline.map((s) => s.name).join(" → ") })}
          </div>
          <button
            onClick={() => setShowEditor(!showEditor)}
            className="text-[10px] text-accent-blue hover:text-accent-blue/80 flex-shrink-0"
          >
            {showEditor ? t("selector.close") : t("selector.edit")}
          </button>
        </div>
        <div className="text-surface-muted text-[10px] mt-1">
          {t("selector.ide")}: <span className="text-accent-blue">{t(`topbar.ideMode.${ideMode}`)}</span>{" "}
          · {t("selector.permission")}: <span className="text-accent-blue">{t(`mode.${mode}`)}</span>
        </div>
      </div>

      {/* Pipeline Editor */}
      {showEditor && (
        <div className="mt-3 border-t border-surface-border pt-2">
          <PipelineEditor />
        </div>
      )}
    </div>
  );
}
