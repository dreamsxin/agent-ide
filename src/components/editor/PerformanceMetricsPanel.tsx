import type { PerformanceMetrics } from "../../utils/incrementalRenderer";

interface PerformanceMetricsPanelProps {
  metrics: PerformanceMetrics | null;
  onReset?: () => void;
}

export default function PerformanceMetricsPanel({ metrics, onReset }: PerformanceMetricsPanelProps) {
  if (!metrics) return null;

  const memory = metrics.memoryUsage >= 1024
    ? `${(metrics.memoryUsage / 1024).toFixed(1)} KB`
    : `${metrics.memoryUsage.toFixed(0)} B`;

  return (
    <div className="absolute right-2 top-2 z-10 w-52 rounded border border-surface-border bg-surface-panel/95 p-2 text-[10px] shadow-lg backdrop-blur-sm">
      <div className="mb-1 flex items-center justify-between text-[11px] font-semibold text-surface-text">
        <span>Editor performance</span>
        <button type="button" onClick={onReset} title="Reset performance metrics" className="text-surface-muted hover:text-surface-text">Reset</button>
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-surface-muted">
        <Metric label="FPS" value={metrics.fps.toFixed(1)} />
        <Metric label="Frame" value={`${metrics.frameTime.toFixed(1)} ms`} />
        <Metric label="Render" value={`${metrics.renderTime.toFixed(1)} ms`} />
        <Metric label="Memory" value={memory} />
        <Metric label="Dropped" value={String(metrics.droppedFrames)} />
        <Metric label="Frames" value={String(metrics.totalFrames)} />
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
