import { useState, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useT } from "../../i18n";
import { AGENT_ROLES, roleIcon, roleLabelKey } from "./agentRoles";
import type { AgentRole, PipelineStage } from "../../types/agent";

export default function PipelineEditor() {
  const t = useT();
  const pipeline = useAgentStore((s) => s.pipeline);
  const updatePipeline = useAgentStore((s) => s.updatePipeline);
  const resetPipeline = useAgentStore((s) => s.resetPipeline);

  const [stages, setStages] = useState<PipelineStage[]>([...pipeline]);
  const [saved, setSaved] = useState(false);

  // 同步外部变化
  if (pipeline !== stages && !saved) {
    // only sync if not currently editing
  }

  const moveUp = useCallback(
    (index: number) => {
      if (index === 0) return;
      const next = [...stages];
      [next[index - 1], next[index]] = [next[index], next[index - 1]];
      setStages(next);
      setSaved(false);
    },
    [stages]
  );

  const moveDown = useCallback(
    (index: number) => {
      if (index === stages.length - 1) return;
      const next = [...stages];
      [next[index], next[index + 1]] = [next[index + 1], next[index]];
      setStages(next);
      setSaved(false);
    },
    [stages]
  );

  const changeRole = useCallback(
    (index: number, role: AgentRole) => {
      const next = [...stages];
      next[index] = { ...next[index], role };
      setStages(next);
      setSaved(false);
    },
    [stages]
  );

  const changeName = useCallback(
    (index: number, name: string) => {
      const next = [...stages];
      next[index] = { ...next[index], name };
      setStages(next);
      setSaved(false);
    },
    [stages]
  );

  const togglePauseBefore = useCallback(
    (index: number) => {
      const next = [...stages];
      next[index] = { ...next[index], pauseBefore: !next[index].pauseBefore };
      setStages(next);
      setSaved(false);
    },
    [stages]
  );

  const removeStage = useCallback(
    (index: number) => {
      if (stages.length <= 1) return;
      setStages(stages.filter((_, i) => i !== index));
      setSaved(false);
    },
    [stages]
  );

  const addStage = useCallback(() => {
    setStages([
      ...stages,
      {
        role: "coder" as AgentRole,
        name: t("pipeline.newStage"),
        status: "pending" as const,
        pauseBefore: false,
      },
    ]);
    setSaved(false);
  }, [stages, t]);

  const handleSave = useCallback(async () => {
    const withPending = stages.map((s) => ({ ...s, status: "pending" as const }));
    await updatePipeline(withPending);
    setStages(withPending);
    setSaved(true);
  }, [stages, updatePipeline]);

  const handleReset = useCallback(async () => {
    await resetPipeline();
    const current = useAgentStore.getState().pipeline;
    setStages([...current]);
    setSaved(true);
  }, [resetPipeline]);

  return (
    <div className="p-3 text-xs overflow-auto h-full">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide flex items-center justify-between">
        <span>{t("pipeline.title")}</span>
        <span className="text-[10px] font-normal">
          {stages.length === 1
            ? t("pipeline.stages.one")
            : t("pipeline.stages.many", { count: stages.length })}
        </span>
      </div>

      {/* 阶段列表 */}
      <div className="space-y-2 mb-3">
        {stages.map((stage, i) => {
          return (
            <div
              key={`${stage.role}-${i}`}
              className="flex items-center gap-1.5 p-2 rounded border border-surface-border bg-surface-base"
            >
              {/* 序号 */}
              <span className="text-[10px] text-surface-muted w-4 text-center flex-shrink-0">
                {i + 1}
              </span>

              {/* 角色选择 */}
              <select
                value={stage.role}
                onChange={(e) => changeRole(i, e.target.value as AgentRole)}
                className="flex-1 min-w-0 px-1.5 py-1 rounded bg-surface-panel border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
              >
                {AGENT_ROLES.map((r) => (
                  <option key={r.id} value={r.id}>
                    {roleIcon(r.id)} {t(roleLabelKey(r.id))}
                  </option>
                ))}
              </select>

              {/* 名称 */}
              <input
                type="text"
                value={stage.name}
                onChange={(e) => changeName(i, e.target.value)}
                className="w-20 px-1.5 py-1 rounded bg-surface-panel border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
              />

              <label className="flex items-center gap-1 text-[10px] text-surface-muted">
                <input
                  type="checkbox"
                  checked={Boolean(stage.pauseBefore)}
                  onChange={() => togglePauseBefore(i)}
                  className="h-3 w-3 accent-accent-blue"
                />
                {t("pipeline.pause")}
              </label>

              {/* 操作 */}
              <div className="flex gap-0.5 flex-shrink-0">
                <button
                  onClick={() => moveUp(i)}
                  disabled={i === 0}
                  className="text-surface-muted hover:text-surface-text disabled:opacity-30 p-0.5 text-[10px]"
                  title={t("pipeline.moveUp")}
                >
                  ▲
                </button>
                <button
                  onClick={() => moveDown(i)}
                  disabled={i === stages.length - 1}
                  className="text-surface-muted hover:text-surface-text disabled:opacity-30 p-0.5 text-[10px]"
                  title={t("pipeline.moveDown")}
                >
                  ▼
                </button>
                <button
                  onClick={() => removeStage(i)}
                  disabled={stages.length <= 1}
                  className="text-diff-remove hover:text-diff-remove/80 disabled:opacity-30 p-0.5 text-[10px]"
                  title={t("pipeline.remove")}
                >
                  ✕
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* 添加阶段 */}
      <button
        onClick={addStage}
        className="w-full mb-3 py-1.5 rounded border border-dashed border-surface-border text-surface-muted hover:text-surface-text hover:border-surface-text/40 text-xs transition-colors"
      >
        {t("pipeline.add")}
      </button>

      {/* 操作按钮 */}
      <div className="flex gap-2">
        <button
          onClick={handleSave}
          disabled={saved}
          className="flex-1 py-1.5 rounded bg-accent-blue hover:bg-accent-blue/80 text-white text-xs font-medium disabled:opacity-50 transition-colors"
        >
          {saved ? t("pipeline.saved") : t("pipeline.save")}
        </button>
        <button
          onClick={handleReset}
          className="flex-1 py-1.5 rounded border border-surface-border text-surface-text hover:bg-surface-border/20 text-xs transition-colors"
        >
          {t("pipeline.reset")}
        </button>
      </div>
    </div>
  );
}
