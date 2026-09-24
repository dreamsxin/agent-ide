import type { PerformanceMetrics } from "../../utils/incrementalRenderer";
import { useT } from "../../i18n";

interface PerformanceMetricsPanelProps {
  metrics: PerformanceMetrics | null;
  onReset?: () => void;
  onClose?: () => void;
}

export default function PerformanceMetricsPanel({ metrics, onReset, onClose }: PerformanceMetricsPanelProps) {
  const t = useT();
  if (!metrics) return null;

  const memory = metrics.memoryUsage >= 1024
    ? `${(metrics.memoryUsage / 1024).toFixed(1)} KB`
    : `${metrics.memoryUsage.toFixed(0)} B`;

  return (
    <div className="absolute right-2 top-2 z-10 w-52 rounded border border-surface-border bg-surface-panel/95 p-2 text-[10px] shadow-lg backdrop-blur-sm">
      <div className="mb-1 flex items-center gap-2 text-[11px] font-semibold text-surface-text">
        <span>{t("perf.title")}</span>
        <button type="button" onClick={onReset} title={t("perf.reset.title")} className="ml-auto text-surface-muted hover:text-surface-text">{t("perf.reset")}</button>
        <button
          type="button"
          onClick={onClose}
          title={t("perf.close")}
          aria-label={t("perf.close")}
          className="text-surface-muted hover:text-surface-text"
        >
          ×
        </button>
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-surface-muted">
        <Metric label={t("perf.fps")} value={metrics.fps.toFixed(1)} />
        <Metric label={t("perf.frame")} value={`${metrics.frameTime.toFixed(1)} ms`} />
        <Metric label={t("perf.render")} value={`${metrics.renderTime.toFixed(1)} ms`} />
        <Metric label={t("perf.memory")} value={memory} />
        <Metric label={t("perf.dropped")} value={String(metrics.droppedFrames)} />
        <Metric label={t("perf.frames")} value={String(metrics.totalFrames)} />
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-2">
      <span>{label}</span>
      <span className="font-mono text-surface-text">{value}</span>
    </div>
  );
}
