import { useState, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { AgentRole, PipelineStage } from "../../types/agent";

const ROLE_LABELS: Record<AgentRole, { label: string; icon: string }> = {
  architect: { label: "Architect", icon: "🏗" },
  coder: { label: "Coder", icon: "💻" },
  tester: { label: "Tester", icon: "🧪" },
  reviewer: { label: "Reviewer", icon: "🔍" },
};

const ALL_ROLES: AgentRole[] = ["architect", "coder", "tester", "reviewer"];

export default function PipelineEditor() {
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
      { role: "coder" as AgentRole, name: "New Stage", status: "pending" as const },
    ]);
    setSaved(false);
  }, [stages]);

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
        <span>Pipeline Editor</span>
        <span className="text-[10px] font-normal">
          {stages.length} stage{stages.length !== 1 ? "s" : ""}
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
                {ALL_ROLES.map((r) => (
                  <option key={r} value={r}>
                    {ROLE_LABELS[r].icon} {ROLE_LABELS[r].label}
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

              {/* 操作 */}
              <div className="flex gap-0.5 flex-shrink-0">
                <button
                  onClick={() => moveUp(i)}
                  disabled={i === 0}
                  className="text-surface-muted hover:text-surface-text disabled:opacity-30 p-0.5 text-[10px]"
                  title="Move up"
                >
                  ▲
                </button>
                <button
                  onClick={() => moveDown(i)}
                  disabled={i === stages.length - 1}
                  className="text-surface-muted hover:text-surface-text disabled:opacity-30 p-0.5 text-[10px]"
                  title="Move down"
                >
                  ▼
                </button>
                <button
                  onClick={() => removeStage(i)}
                  disabled={stages.length <= 1}
                  className="text-diff-remove hover:text-diff-remove/80 disabled:opacity-30 p-0.5 text-[10px]"
                  title="Remove"
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
        + Add Stage
      </button>

      {/* 操作按钮 */}
      <div className="flex gap-2">
        <button
          onClick={handleSave}
          disabled={saved}
          className="flex-1 py-1.5 rounded bg-accent-blue hover:bg-accent-blue/80 text-white text-xs font-medium disabled:opacity-50 transition-colors"
        >
          {saved ? "Saved ✓" : "Save Pipeline"}
        </button>
        <button
          onClick={handleReset}
          className="flex-1 py-1.5 rounded border border-surface-border text-surface-text hover:bg-surface-border/20 text-xs transition-colors"
        >
          Reset Default
        </button>
      </div>
    </div>
  );
}
