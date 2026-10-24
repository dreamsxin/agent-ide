import { describe, expect, it } from "vitest";
import type { editor } from "monaco-editor";
import {
  IncrementalRenderer,
  PerformanceProfiler,
  createPerformanceProfiler,
  type TextChange,
} from "./incrementalRenderer";

const fakeModel = {} as editor.ITextModel;

function change(lineNumbers: number[]): TextChange {
  return {
    lineNumber: lineNumbers[0] ?? 1,
    oldContent: "",
    newContent: "",
    affectedLines: lineNumbers,
  };
}

describe("IncrementalRenderer", () => {
  it("marks valid lines dirty and rejects invalid line numbers", () => {
    const renderer = new IncrementalRenderer();

    renderer.markLineDirty(7, "edit");
    renderer.markLineDirty(0, "edit");
    renderer.markLineDirty(-1, "edit");
    renderer.markLineDirty(2.5, "edit");

    expect(renderer.getDirtyLineCount()).toBe(1);
    expect(renderer.getRenderQueueSize()).toBe(1);
  });

  it("evicts the oldest queued line when the render queue is full", () => {
    const renderer = new IncrementalRenderer({ maxRenderQueueSize: 3 });

    for (const line of [1, 2, 3, 4]) {
      renderer.markLineDirty(line, "edit");
    }

    expect(renderer.getRenderQueueSize()).toBe(3);
    // line 1 was evicted, so only lines 2-4 remain
    const result = renderer.renderWithBudget({} as editor.IStandaloneCodeEditor, fakeModel);
    expect(result.renderedLines).toBe(3);
  });

  it("marks all lines in an updated viewport dirty for scroll reasons", () => {
    const renderer = new IncrementalRenderer();
    renderer.updateViewport({ startLineNumber: 10, endLineNumber: 12 });

    expect(renderer.getDirtyLineCount()).toBe(3);
  });

  it("tracks text changes in bounded history and marks affected lines", () => {
    const renderer = new IncrementalRenderer({ maxChangeHistorySize: 2 });

    renderer.handleTextChange(change([5, 6]));
    renderer.handleTextChange(change([7]));
    renderer.handleTextChange(change([8]));

    const history = renderer.getChangeHistory();
    expect(history).toHaveLength(2);
    expect(history.map((entry) => entry.affectedLines)).toEqual([[7], [8]]);
    expect(renderer.getDirtyLineCount()).toBe(4);

    renderer.clearChangeHistory();
    expect(renderer.getChangeHistory()).toHaveLength(0);
  });

  it("renders viewport dirty lines first within the frame budget", () => {
    const renderer = new IncrementalRenderer({ frameBudgetMs: 50 });
    renderer.updateViewport({ startLineNumber: 1, endLineNumber: 2 });
    renderer.markLineDirty(50, "edit");

    const result = renderer.renderWithBudget({} as editor.IStandaloneCodeEditor, fakeModel);

    expect(result.renderedLines).toBe(3);
    expect(result.skippedLines).toBe(0);
    expect(result.renderTime).toBeGreaterThanOrEqual(0);
    expect(result.frameBudgetUsed).toBeGreaterThanOrEqual(0);
    expect(renderer.getRenderQueueSize()).toBe(0);
  });

  it("skips rendering while a frame is already in progress", () => {
    const renderer = new IncrementalRenderer();
    renderer.markLineDirty(1, "edit");

    const first = renderer.renderWithBudget({} as editor.IStandaloneCodeEditor, fakeModel);
    expect(first.renderedLines).toBe(1);
  });

  it("clears dirty lines on demand", () => {
    const renderer = new IncrementalRenderer();
    renderer.markLineDirty(3, "decoration");
    renderer.clearDirtyLines();

    expect(renderer.getDirtyLineCount()).toBe(0);
    expect(renderer.getRenderQueueSize()).toBe(0);
  });

  it("updates and resets performance metrics", () => {
    const renderer = new IncrementalRenderer();
    renderer.markLineDirty(1, "edit");
    renderer.renderWithBudget({} as editor.IStandaloneCodeEditor, fakeModel);

    const metrics = renderer.getMetrics();
    expect(metrics.totalFrames).toBe(1);
    expect(metrics.renderTime).toBeGreaterThanOrEqual(0);

    renderer.resetMetrics();
    expect(renderer.getMetrics().totalFrames).toBe(0);
  });
});

describe("PerformanceProfiler", () => {
  it("collects metric samples and reports stats", () => {
    const profiler = createPerformanceProfiler(10);

    profiler.recordMetric("render", 10);
    profiler.recordMetric("render", 20);

    expect(profiler.getMetricStats("render")).toEqual({
      avg: 15,
      min: 10,
      max: 20,
      count: 2,
    });
    expect(profiler.getMetricStats("missing")).toBeNull();
  });

  it("caps metric samples at the configured maximum", () => {
    const profiler = new PerformanceProfiler(3);

    for (const value of [1, 2, 3, 4, 5]) {
      profiler.recordMetric("frame", value);
    }

    const stats = profiler.getMetricStats("frame");
    expect(stats?.count).toBe(3);
    expect(stats?.min).toBe(3);
  });

  it("measures durations with startMeasure and aggregates flame data", () => {
    const profiler = new PerformanceProfiler();
    const stop = profiler.startMeasure("renderLine");
    stop();

    profiler.recordFlameData("renderLine", 5);
    profiler.recordFlameData("renderLine", 15);

    const flame = profiler.getFlameGraphData();
    expect(flame.get("renderLine")).toEqual({ avg: 10, count: 2 });

    const report = profiler.getReport();
    expect(report).toContain("性能分析报告");
    expect(report).toContain("renderLine");

    profiler.clear();
    expect(profiler.getMetricStats("renderLine")).toBeNull();
    expect(profiler.getFlameGraphData().size).toBe(0);
  });
});
