/**
 * 增量渲染 React Hook
 * 集成增量渲染引擎到 Monaco 编辑器
 */

import { useEffect, useRef, useCallback, useState } from "react";
import type { editor, IScrollEvent } from "monaco-editor";
import {
  IncrementalRenderer,
  TextChangeDetector,
  PerformanceProfiler,
  createIncrementalRenderer,
  createPerformanceProfiler,
  type IncrementalRendererConfig,
  type PerformanceMetrics,
} from "../utils/incrementalRenderer";

/**
 * 增量渲染 Hook 配置
 */
export interface UseIncrementalRenderingOptions {
  enabled?: boolean;
  config?: Partial<IncrementalRendererConfig>;
  onMetricsUpdate?: (metrics: PerformanceMetrics) => void;
  profilingEnabled?: boolean;
}

/**
 * 增量渲染 Hook 返回值
 */
export interface UseIncrementalRenderingReturn {
  renderer: IncrementalRenderer | null;
  profiler: PerformanceProfiler | null;
  metrics: PerformanceMetrics | null;
  getMetrics: () => PerformanceMetrics | null;
  resetMetrics: () => void;
  forceRender: () => void;
}

/**
 * 增量渲染 Hook
 */
export function useIncrementalRendering(
  editor: editor.IStandaloneCodeEditor | null,
  options: UseIncrementalRenderingOptions = {}
): UseIncrementalRenderingReturn {
  const {
    enabled = true,
    config,
    onMetricsUpdate,
    profilingEnabled = false,
  } = options;

  const rendererRef = useRef<IncrementalRenderer | null>(null);
  const profilerRef = useRef<PerformanceProfiler | null>(null);
  const metricsRef = useRef<PerformanceMetrics | null>(null);
  const onMetricsUpdateRef = useRef(onMetricsUpdate);
  const configRef = useRef(config);
  configRef.current = config;
  const [renderer, setRenderer] = useState<IncrementalRenderer | null>(null);
  const [profiler, setProfiler] = useState<PerformanceProfiler | null>(null);
  const [metrics, setMetrics] = useState<PerformanceMetrics | null>(null);
  const contentRef = useRef<string>("");

  onMetricsUpdateRef.current = onMetricsUpdate;
  const disposablesRef = useRef<Set<{ dispose(): void }>>(new Set());
  const animationFrameRef = useRef<number | null>(null);
  const metricsUpdateIntervalRef = useRef<number | null>(null);

  // 初始化渲染器和性能分析器
  useEffect(() => {
    if (!enabled || !editor) {
      return;
    }

    // 创建增量渲染器
    rendererRef.current = createIncrementalRenderer(configRef.current);
    setRenderer(rendererRef.current);

    // 创建性能分析器（如果启用）
    if (profilingEnabled) {
      profilerRef.current = createPerformanceProfiler(configRef.current?.maxProfilerSamples);
      setProfiler(profilerRef.current);
    }

    // 设置编辑器变更监听
    const model = editor.getModel();
    if (model) {
      contentRef.current = model.getValue();

      // 监听内容变更
      const changeDisposable = model.onDidChangeContent(() => {
        const endMeasure = profilerRef.current?.startMeasure("content-change");
        handleContentChange(model);
        endMeasure?.();
      });
      disposablesRef.current.add(changeDisposable);

      // 监听滚动事件
      const scrollDisposable = editor.onDidScrollChange((e) => {
        const endMeasure = profilerRef.current?.startMeasure("scroll-change");
        handleScrollChange(e);
        endMeasure?.();
      });
      disposablesRef.current.add(scrollDisposable);

      // 监听视口变更
      const viewportChangeDisposable = editor.onDidLayoutChange(() => {
        const endMeasure = profilerRef.current?.startMeasure("viewport-change");
        handleViewportChange();
        endMeasure?.();
      });
      disposablesRef.current.add(viewportChangeDisposable);

      // 初始化视口
      handleViewportChange();
    }

    // 启动渲染循环
    startRenderLoop();

    // 启动性能指标更新循环
    metricsUpdateIntervalRef.current = window.setInterval(() => {
      updateMetrics();
    }, 1000); // 每秒更新一次

    return () => {
      // 清理
      cleanup();
    };
  }, [editor, enabled, profilingEnabled]);

  // 处理内容变更
  const handleContentChange = useCallback(
    (model: editor.ITextModel) => {
      const renderer = rendererRef.current;
      if (!renderer) return;

      const newContent = model.getValue();
      const oldContent = contentRef.current;

      // 检测变更
      const detectedChanges = TextChangeDetector.detectChanges(oldContent, newContent);

      // 使用前后内容差异作为变更来源，避免从已更新的 Monaco model 读取旧文本。
      for (const change of detectedChanges) {
        renderer.handleTextChange({
          ...change,
          affectedLines: TextChangeDetector.detectAffectedLines(change, 2),
        });
      }

      contentRef.current = newContent;
    },
    []
  );

  // 处理滚动变更
  const handleScrollChange = useCallback(
    (e: IScrollEvent) => {
      const renderer = rendererRef.current;
      if (!renderer) return;

      renderer.updateViewport({
        scrollTop: e.scrollTop,
        scrollLeft: e.scrollLeft,
      });
    },
    []
  );

  // 处理视口变更
  const handleViewportChange = useCallback(() => {
    const activeRenderer = rendererRef.current;
    if (!activeRenderer || !editor) return;

    const layoutInfo = editor.getLayoutInfo();
    const visibleRanges = editor.getVisibleRanges();

    if (visibleRanges.length > 0) {
      const firstRange = visibleRanges[0];
      const lastRange = visibleRanges[visibleRanges.length - 1];

      activeRenderer.updateViewport({
        startLineNumber: firstRange.startLineNumber,
        endLineNumber: lastRange.endLineNumber,
        width: layoutInfo.width,
        height: layoutInfo.height,
      });
    }
  }, [editor]);

  // 启动渲染循环
  const startRenderLoop = useCallback(() => {
    const renderFrame = () => {
      if (!editor || !rendererRef.current) {
        return;
      }

      const model = editor.getModel();
      if (!model) {
        animationFrameRef.current = requestAnimationFrame(renderFrame);
        return;
      }

      const endMeasure = profilerRef.current?.startMeasure("render-frame");
      const result = rendererRef.current.renderWithBudget(editor, model);
      endMeasure?.();

      // 记录渲染性能
      if (profilerRef.current) {
        profilerRef.current.recordMetric("render-time", result.renderTime);
        profilerRef.current.recordMetric("rendered-lines", result.renderedLines);
        profilerRef.current.recordMetric("skipped-lines", result.skippedLines);
      }

      // 更新指标
      const metrics = rendererRef.current.getMetrics();
      metricsRef.current = metrics;
      setMetrics(metrics);

      // 继续下一帧
      animationFrameRef.current = requestAnimationFrame(renderFrame);
    };

    animationFrameRef.current = requestAnimationFrame(renderFrame);
  }, [editor]);

  // 更新性能指标
  const updateMetrics = useCallback(() => {
    const activeRenderer = rendererRef.current;
    if (!activeRenderer) return;

    const nextMetrics = activeRenderer.getMetrics();
    metricsRef.current = nextMetrics;
    setMetrics(nextMetrics);
    onMetricsUpdateRef.current?.(nextMetrics);
  }, []);

  // 清理函数
  const cleanup = useCallback(() => {
    // 停止渲染循环
    if (animationFrameRef.current !== null) {
      cancelAnimationFrame(animationFrameRef.current);
      animationFrameRef.current = null;
    }

    // 停止指标更新循环
    if (metricsUpdateIntervalRef.current !== null) {
      clearInterval(metricsUpdateIntervalRef.current);
      metricsUpdateIntervalRef.current = null;
    }

    // 清理监听器
    disposablesRef.current.forEach((disposable) => disposable.dispose());
    disposablesRef.current.clear();

    // 重置渲染器
    rendererRef.current = null;
    profilerRef.current = null;
    metricsRef.current = null;
    setRenderer(null);
    setProfiler(null);
    setMetrics(null);
  }, []);

  // 强制渲染
  const forceRender = useCallback(() => {
    const activeRenderer = rendererRef.current;
    if (!activeRenderer || !editor) return;

    const model = editor.getModel();
    if (!model) return;

    activeRenderer.renderWithBudget(editor, model);
    updateMetrics();
  }, [editor, updateMetrics]);

  // 获取指标
  const getMetrics = useCallback(() => {
    return rendererRef.current?.getMetrics() ?? null;
  }, []);

  // 重置指标
  const resetMetrics = useCallback(() => {
    rendererRef.current?.resetMetrics();
    profilerRef.current?.clear();
    metricsRef.current = null;
    setMetrics(null);
  }, []);

  return {
    renderer,
    profiler,
    metrics,
    getMetrics,
    resetMetrics,
    forceRender,
  };
}

/**
 * 性能监控 Hook
 */
export function usePerformanceMonitor(
  profiler: PerformanceProfiler | null,
  interval: number = 5000
) {
  const [report, setReport] = useState<string>("");

  useEffect(() => {
    if (!profiler) return;

    const updateReport = () => {
      const reportText = profiler.getReport();
      setReport(reportText);
    };

    updateReport();
    const intervalId = setInterval(updateReport, interval);

    return () => clearInterval(intervalId);
  }, [profiler, interval]);

  return { report, refresh: () => setReport(profiler?.getReport() ?? "") };
}

/**
 * 渲染状态 Hook
 */
export function useRenderingStatus(renderer: IncrementalRenderer | null) {
  const [status, setStatus] = useState({
    dirtyLines: 0,
    renderQueueSize: 0,
    isHealthy: true,
  });

  useEffect(() => {
    if (!renderer) return;

    const updateStatus = () => {
      const dirtyLines = renderer.getDirtyLineCount();
      const renderQueueSize = renderer.getRenderQueueSize();

      setStatus({
        dirtyLines,
        renderQueueSize,
        isHealthy: dirtyLines < 100 && renderQueueSize < 50, // 健康阈值
      });
    };

    updateStatus();
    const intervalId = setInterval(updateStatus, 500);

    return () => clearInterval(intervalId);
  }, [renderer]);

  return status;
}