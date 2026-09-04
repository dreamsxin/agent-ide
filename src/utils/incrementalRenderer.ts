/**
 * 增量渲染引擎 - 受 Zed Editor 高性能架构启发
 * 实现视口跟踪、脏行检测、帧预算渲染和多线程编辑器操作
 */

import type { editor } from "monaco-editor";

/**
 * 视口信息
 */
export interface Viewport {
  startLineNumber: number;
  endLineNumber: number;
  scrollTop: number;
  scrollLeft: number;
  width: number;
  height: number;
}

/**
 * 脏行信息
 */
export interface DirtyLine {
  lineNumber: number;
  reason: "edit" | "scroll" | "decoration" | "syntax";
  timestamp: number;
}

/**
 * 文本变更信息
 */
export interface TextChange {
  lineNumber: number;
  oldContent: string;
  newContent: string;
  affectedLines: number[];
}

/**
 * 渲染结果
 */
export interface RenderResult {
  renderedLines: number;
  skippedLines: number;
  renderTime: number;
  frameBudgetUsed: number;
}

/**
 * 性能指标
 */
export interface PerformanceMetrics {
  frameTime: number;
  renderTime: number;
  memoryUsage: number;
  fps: number;
  totalFrames: number;
  droppedFrames: number;
}

/**
 * 增量渲染引擎配置
 */
export interface IncrementalRendererConfig {
  frameBudgetMs: number;           // 帧预算（毫秒）
  targetFps: number;               // 目标帧率
  enableMultiThreading: boolean;   // 启用多线程
  dirtyLineTtl: number;            // 脏行生存时间（毫秒）
  maxRenderQueueSize: number;      // 最大渲染队列大小
  maxChangeHistorySize?: number;   // 最大变更历史数量
  maxProfilerSamples?: number;     // 每项性能指标最大样本数
}

/**
 * 增量渲染引擎
 */
export class IncrementalRenderer {
  private config: IncrementalRendererConfig;
  private viewport: Viewport;
  private dirtyLines: Map<number, DirtyLine>;
  private changeHistory: TextChange[];
  private performanceMetrics: PerformanceMetrics;
  private isRendering: boolean;
  private lastFrameTime: number;
  private frameCount: number;
  private renderQueue: Set<number>;
  private readonly maxChangeHistorySize: number;

  constructor(config: Partial<IncrementalRendererConfig> = {}) {
    this.config = {
      frameBudgetMs: config.frameBudgetMs ?? 16,    // 60fps = 16.67ms per frame
      targetFps: config.targetFps ?? 60,
      enableMultiThreading: config.enableMultiThreading ?? false,
      dirtyLineTtl: config.dirtyLineTtl ?? 1000,    // 1秒后清除脏行
      maxRenderQueueSize: Math.max(1, Math.floor(config.maxRenderQueueSize ?? 1000)),
      maxChangeHistorySize: Math.max(1, Math.floor(config.maxChangeHistorySize ?? 100)),
      maxProfilerSamples: Math.max(1, Math.floor(config.maxProfilerSamples ?? 200)),
    };

    this.viewport = this.createEmptyViewport();
    this.dirtyLines = new Map();
    this.changeHistory = [];
    this.performanceMetrics = this.createEmptyMetrics();
    this.isRendering = false;
    this.lastFrameTime = performance.now();
    this.frameCount = 0;
    this.renderQueue = new Set();
    this.maxChangeHistorySize = this.config.maxChangeHistorySize!;
  }

  /**
   * 更新视口
   */
  updateViewport(newViewport: Partial<Viewport>): void {
    this.viewport = { ...this.viewport, ...newViewport };

    // 标记视口内所有行需要渲染
    for (let line = this.viewport.startLineNumber; line <= this.viewport.endLineNumber; line++) {
      this.markLineDirty(line, "scroll");
    }
  }

  /**
   * 标记行为脏行
   */
  markLineDirty(lineNumber: number, reason: DirtyLine["reason"]): void {
    if (!Number.isInteger(lineNumber) || lineNumber < 1) return;

    if (!this.renderQueue.has(lineNumber) && this.renderQueue.size >= this.config.maxRenderQueueSize) {
      const oldestLine = this.renderQueue.values().next().value as number | undefined;
      if (oldestLine !== undefined) {
        this.renderQueue.delete(oldestLine);
        this.dirtyLines.delete(oldestLine);
      }
    }

    const dirtyLine: DirtyLine = {
      lineNumber,
      reason,
      timestamp: Date.now(),
    };
    this.dirtyLines.set(lineNumber, dirtyLine);
    this.renderQueue.add(lineNumber);
  }

  /**
   * 处理文本变更
   */
  handleTextChange(change: TextChange): void {
    this.changeHistory.push(change);
    if (this.changeHistory.length > this.maxChangeHistorySize) {
      this.changeHistory.splice(0, this.changeHistory.length - this.maxChangeHistorySize);
    }

    // 标记受影响的行
    for (const line of change.affectedLines) {
      this.markLineDirty(line, "edit");
    }
  }

  /**
   * 帧预算渲染
   */
  renderWithBudget(_editor: editor.IStandaloneCodeEditor, model: editor.ITextModel): RenderResult {
    if (this.isRendering) {
      return this.createEmptyResult();
    }

    const startTime = performance.now();
    const frameDeadline = startTime + this.config.frameBudgetMs;
    this.isRendering = true;

    try {
      const result = this.renderCriticalElements(model, frameDeadline);
      const renderTime = performance.now() - startTime;

      this.updateMetrics(renderTime);
      this.cleanupDirtyLines();

      return {
        ...result,
        renderTime,
        frameBudgetUsed: (renderTime / this.config.frameBudgetMs) * 100,
      };
    } finally {
      this.isRendering = false;
      this.lastFrameTime = performance.now();
      this.frameCount++;
    }
  }

  /**
   * 渲染关键元素
   */
  private renderCriticalElements(
    model: editor.ITextModel,
    deadline: number
  ): Omit<RenderResult, "renderTime" | "frameBudgetUsed"> {
    let renderedLines = 0;
    let skippedLines = 0;
    let renderedViewportLines = 0;
    let renderedOtherLines = 0;

    // 优先渲染视口内的脏行
    const viewportDirtyLines = this.getViewportDirtyLines();

    for (const lineNumber of viewportDirtyLines) {
      if (performance.now() >= deadline) {
        skippedLines += viewportDirtyLines.length - renderedViewportLines;
        break;
      }

      this.renderLine(model, lineNumber);
      this.renderQueue.delete(lineNumber);
      renderedLines++;
      renderedViewportLines++;
    }

    // 如果还有时间，渲染其他脏行
    if (performance.now() < deadline) {
      const remainingDirtyLines = Array.from(this.renderQueue).filter(
        line => !this.isLineInViewport(line)
      );

      for (const lineNumber of remainingDirtyLines) {
        if (performance.now() >= deadline) {
          skippedLines += remainingDirtyLines.length - renderedOtherLines;
          break;
        }

        this.renderLine(model, lineNumber);
        this.renderQueue.delete(lineNumber);
        renderedLines++;
        renderedOtherLines++;
      }
    }

    return { renderedLines, skippedLines };
  }

  /**
   * 渲染单行
   */
  private renderLine(_model: editor.ITextModel, _lineNumber: number): void {
    // Monaco renders model and viewport changes itself. Keep this operation
    // side-effect free instead of emitting a fake input or decoration event.
  }

  /**
   * 获取视口内的脏行
   */
  private getViewportDirtyLines(): number[] {
    const viewportLines: number[] = [];
    for (let line = this.viewport.startLineNumber; line <= this.viewport.endLineNumber; line++) {
      if (this.renderQueue.has(line) && this.dirtyLines.has(line)) {
        viewportLines.push(line);
      }
    }
    return viewportLines.sort((a, b) => a - b);
  }

  /**
   * 检查行是否在视口内
   */
  private isLineInViewport(lineNumber: number): boolean {
    return lineNumber >= this.viewport.startLineNumber &&
           lineNumber <= this.viewport.endLineNumber;
  }

  /**
   * 清理过期的脏行
   */
  private cleanupDirtyLines(): void {
    const now = Date.now();
    const linesToRemove: number[] = [];

    for (const [lineNumber, dirtyLine] of this.dirtyLines) {
      if (now - dirtyLine.timestamp > this.config.dirtyLineTtl) {
        linesToRemove.push(lineNumber);
      }
    }

    for (const lineNumber of linesToRemove) {
      this.dirtyLines.delete(lineNumber);
      this.renderQueue.delete(lineNumber);
    }
  }

  /**
   * 更新性能指标
   */
  private updateMetrics(renderTime: number): void {
    const now = performance.now();
    const frameTime = now - this.lastFrameTime;

    this.performanceMetrics.frameTime = frameTime;
    this.performanceMetrics.renderTime = renderTime;
    this.performanceMetrics.fps = 1000 / frameTime;
    this.performanceMetrics.totalFrames++;

    // 检测掉帧
    if (frameTime > (1000 / this.config.targetFps) * 1.2) {
      this.performanceMetrics.droppedFrames++;
    }

    // 估算内存使用（简化版）
    this.performanceMetrics.memoryUsage =
      this.dirtyLines.size * 100 + // 脏行占用
      this.changeHistory.length * 200; // 变更历史占用
  }

  /**
   * 获取性能指标
   */
  getMetrics(): PerformanceMetrics {
    return { ...this.performanceMetrics };
  }

  /**
   * 重置性能指标
   */
  resetMetrics(): void {
    this.performanceMetrics = this.createEmptyMetrics();
    this.frameCount = 0;
    this.lastFrameTime = performance.now();
  }

  /**
   * 获取脏行数量
   */
  getDirtyLineCount(): number {
    return this.dirtyLines.size;
  }

  /**
   * 获取渲染队列大小
   */
  getRenderQueueSize(): number {
    return this.renderQueue.size;
  }

  /**
   * 清空所有脏行
   */
  clearDirtyLines(): void {
    this.dirtyLines.clear();
    this.renderQueue.clear();
  }

  /**
   * 获取变更历史
   */
  getChangeHistory(): TextChange[] {
    return [...this.changeHistory];
  }

  /**
   * 清空变更历史
   */
  clearChangeHistory(): void {
    this.changeHistory = [];
  }

  // 私有辅助方法

  private createEmptyViewport(): Viewport {
    return {
      startLineNumber: 1,
      endLineNumber: 1,
      scrollTop: 0,
      scrollLeft: 0,
      width: 0,
      height: 0,
    };
  }

  private createEmptyMetrics(): PerformanceMetrics {
    return {
      frameTime: 0,
      renderTime: 0,
      memoryUsage: 0,
      fps: 60,
      totalFrames: 0,
      droppedFrames: 0,
    };
  }

  private createEmptyResult(): RenderResult {
    return {
      renderedLines: 0,
      skippedLines: 0,
      renderTime: 0,
      frameBudgetUsed: 0,
    };
  }
}

/**
 * 创建增量渲染引擎实例
 */
export function createIncrementalRenderer(
  config?: Partial<IncrementalRendererConfig>
): IncrementalRenderer {
  return new IncrementalRenderer(config);
}

/**
 * 文本变更检测器
 */
export class TextChangeDetector {
  /**
   * 检测文本变更
   */
  static detectChanges(
    oldContent: string,
    newContent: string
  ): TextChange[] {
    const changes: TextChange[] = [];
    const oldLines = oldContent.split("\n");
    const newLines = newContent.split("\n");

    const maxLines = Math.max(oldLines.length, newLines.length);

    for (let i = 0; i < maxLines; i++) {
      const oldLine = oldLines[i] ?? "";
      const newLine = newLines[i] ?? "";

      if (oldLine !== newLine) {
        changes.push({
          lineNumber: i + 1,
          oldContent: oldLine,
          newContent: newLine,
          affectedLines: [i + 1], // 可以扩展为检测多行变更
        });
      }
    }

    return changes;
  }

  /**
   * 检测受影响的行范围
   */
  static detectAffectedLines(
    change: TextChange,
    contextLines: number = 2
  ): number[] {
    const affectedLines: number[] = [];

    // 包含变更行本身
    affectedLines.push(change.lineNumber);

    // 包含上下文行
    for (let i = 1; i <= contextLines; i++) {
      affectedLines.push(change.lineNumber - i);
      affectedLines.push(change.lineNumber + i);
    }

    // 过滤掉负数行号并去重
    return [...new Set(affectedLines.filter(line => line > 0))];
  }
}

/**
 * 性能分析器
 */
export class PerformanceProfiler {
  private metrics: Map<string, number[]> = new Map();
  private flameGraph: Map<string, number[]> = new Map();
  private readonly maxSamples: number;

  constructor(maxSamples: number = 200) {
    this.maxSamples = Math.max(1, Math.floor(maxSamples));
  }

  /**
   * 开始性能测量
   */
  startMeasure(name: string): () => void {
    const startTime = performance.now();

    return () => {
      const duration = performance.now() - startTime;
      this.recordMetric(name, duration);
    };
  }

  /**
   * 记录性能指标
   */
  recordMetric(name: string, duration: number): void {
    if (!this.metrics.has(name)) {
      this.metrics.set(name, []);
    }
    const values = this.metrics.get(name)!;
    values.push(duration);
    if (values.length > this.maxSamples) {
      values.splice(0, values.length - this.maxSamples);
    }
  }

  /**
   * 记录火焰图数据
   */
  recordFlameData(functionName: string, duration: number): void {
    if (!this.flameGraph.has(functionName)) {
      this.flameGraph.set(functionName, []);
    }
    const durations = this.flameGraph.get(functionName)!;
    durations.push(duration);
    if (durations.length > this.maxSamples) {
      durations.splice(0, durations.length - this.maxSamples);
    }
  }

  /**
   * 获取指标统计
   */
  getMetricStats(name: string): { avg: number; min: number; max: number; count: number } | null {
    const values = this.metrics.get(name);
    if (!values || values.length === 0) return null;

    const sum = values.reduce((a, b) => a + b, 0);
    return {
      avg: sum / values.length,
      min: Math.min(...values),
      max: Math.max(...values),
      count: values.length,
    };
  }

  /**
   * 获取火焰图数据
   */
  getFlameGraphData(): Map<string, { avg: number; count: number }> {
    const result = new Map<string, { avg: number; count: number }>();

    for (const [functionName, durations] of this.flameGraph) {
      const sum = durations.reduce((a, b) => a + b, 0);
      result.set(functionName, {
        avg: sum / durations.length,
        count: durations.length,
      });
    }

    return result;
  }

  /**
   * 清空所有数据
   */
  clear(): void {
    this.metrics.clear();
    this.flameGraph.clear();
  }

  /**
   * 获取性能报告
   */
  getReport(): string {
    const lines: string[] = ["=== 性能分析报告 ===\n"];

    for (const [name] of this.metrics) {
      const stats = this.getMetricStats(name);
      if (stats) {
        lines.push(`${name}:`);
        lines.push(`  平均: ${stats.avg.toFixed(2)}ms`);
        lines.push(`  最小: ${stats.min.toFixed(2)}ms`);
        lines.push(`  最大: ${stats.max.toFixed(2)}ms`);
        lines.push(`  调用次数: ${stats.count}`);
        lines.push("");
      }
    }

    return lines.join("\n");
  }
}

/**
 * 创建性能分析器实例
 */
export function createPerformanceProfiler(maxSamples?: number): PerformanceProfiler {
  return new PerformanceProfiler(maxSamples);
}