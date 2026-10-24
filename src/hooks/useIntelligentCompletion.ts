/**
 * 智能代码补全 React Hook
 * 集成智能补全系统到 Monaco 编辑器
 */

import { useEffect, useRef, useCallback, useState } from "react";
import type { editor } from "monaco-editor";
import {
  IntelligentCompletionEngine,
  createIntelligentCompletionEngine,
  createMonacoCompletionProvider,
  type CompletionSuggestion,
  type CodeContext,
} from "../utils/intelligentCompletion";

/**
 * 智能补全 Hook 配置
 */
export interface UseIntelligentCompletionOptions {
  enabled?: boolean;
  cacheEnabled?: boolean;
  maxSuggestions?: number;
  onSuggestionGenerated?: (suggestions: CompletionSuggestion[]) => void;
  onContextAnalyzed?: (context: CodeContext) => void;
}

/**
 * 智能补全 Hook 返回值
 */
export interface UseIntelligentCompletionReturn {
  engine: IntelligentCompletionEngine | null;
  currentSuggestions: CompletionSuggestion[];
  lastContext: CodeContext | null;
  getContext: (file: string, position: { lineNumber: number; column: number }, surroundingCode: string) => Promise<CodeContext>;
  forceRefresh: () => void;
  clearCache: () => void;
  setCacheEnabled: (enabled: boolean) => void;
  statistics: {
    totalSuggestions: number;
    cacheHits: number;
    averageConfidence: number;
  };
}

/**
 * 智能补全 Hook
 */
export function useIntelligentCompletion(
  editor: editor.IStandaloneCodeEditor | null,
  monaco: typeof import("monaco-editor") | null,
  options: UseIntelligentCompletionOptions = {}
): UseIntelligentCompletionReturn {
  // maxSuggestions / onSuggestionGenerated 属于 Hook 的公开选项，
  // 但要等 Monaco provider 把生成结果回填到 currentSuggestionsRef 之后才能生效，
  // 因此这里暂不解构，避免出现未使用的绑定。
  const { enabled = true, cacheEnabled = true, onContextAnalyzed } = options;

  const engineRef = useRef<IntelligentCompletionEngine | null>(null);
  const currentSuggestionsRef = useRef<CompletionSuggestion[]>([]);
  const lastContextRef = useRef<CodeContext | null>(null);
  const disposablesRef = useRef<Set<{ dispose(): void }>>(new Set());
  const statisticsRef = useRef({
    totalSuggestions: 0,
    cacheHits: 0,
    totalConfidence: 0,
    averageConfidence: 0,
  });

  const [currentSuggestions, setCurrentSuggestions] = useState<CompletionSuggestion[]>([]);
  const [lastContext, setLastContext] = useState<CodeContext | null>(null);

  // 初始化智能补全引擎
  useEffect(() => {
    if (!enabled) {
      return;
    }

    // 创建智能补全引擎
    engineRef.current = createIntelligentCompletionEngine();
    engineRef.current.setCacheEnabled(cacheEnabled);

    return () => {
      cleanup();
    };
  }, [enabled, cacheEnabled]);

  // 注册补全提供者
  useEffect(() => {
    if (!enabled || !editor || !monaco || !engineRef.current) {
      return;
    }

    // 为支持的语言注册智能补全提供者
    const supportedLanguages = [
      'typescript', 'javascript', 'python', 'rust', 'go',
      'java', 'cpp', 'c', 'html', 'css', 'json', 'markdown'
    ];

    for (const language of supportedLanguages) {
      const provider = createMonacoCompletionProvider(engineRef.current, monaco);
      const disposable = monaco.languages.registerCompletionItemProvider(language, provider);
      disposablesRef.current.add(disposable);
    }

    return () => {
      disposablesRef.current.forEach(d => d.dispose());
      disposablesRef.current.clear();
    };
  }, [editor, monaco, enabled]);

  // 监听编辑器变化以更新统计信息
  useEffect(() => {
    if (!editor || !engineRef.current) {
      return;
    }

    const disposable = editor.onDidChangeModelContent(() => {
      updateStatistics();
    });

    disposablesRef.current.add(disposable);

    return () => {
      disposable.dispose();
    };
  }, [editor]);

  // 获取代码上下文
  const getContext = useCallback(async (
    file: string,
    position: { lineNumber: number; column: number },
    surroundingCode: string,
    language?: string
  ): Promise<CodeContext> => {
    const engine = engineRef.current;
    if (!engine) {
      throw new Error("智能补全引擎未初始化");
    }

    const context = await engine.getContextAnalyzer().analyze({
      file,
      position,
      surroundingCode,
      language,
    });

    lastContextRef.current = context;
    setLastContext(context);

    if (onContextAnalyzed) {
      onContextAnalyzed(context);
    }

    return context;
  }, [onContextAnalyzed]);

  // 强制刷新
  const forceRefresh = useCallback(() => {
    if (!editor || !engineRef.current) {
      return;
    }

    const model = editor.getModel();
    if (!model) {
      return;
    }

    const position = editor.getPosition();
    if (!position) {
      return;
    }

    // 触发补全重新计算
    editor.trigger("keyboard", "editor.action.triggerSuggest", {});

    // 清除当前建议
    currentSuggestionsRef.current = [];
    setCurrentSuggestions([]);
  }, [editor]);

  // 清除缓存
  const clearCache = useCallback(() => {
    engineRef.current?.clearCache();
    engineRef.current?.getContextAnalyzer().clearCache();
    currentSuggestionsRef.current = [];
    setCurrentSuggestions([]);
    // 重置统计信息
    statisticsRef.current = {
      totalSuggestions: 0,
      cacheHits: 0,
      totalConfidence: 0,
      averageConfidence: 0,
    };
  }, []);

  // 设置缓存启用状态
  const setCacheEnabled = useCallback((enabled: boolean) => {
    engineRef.current?.setCacheEnabled(enabled);
  }, []);

  // 更新统计信息
  const updateStatistics = useCallback(() => {
    const stats = statisticsRef.current;
    const suggestions = currentSuggestionsRef.current;

    if (suggestions.length > 0) {
      const avgConfidence = suggestions.reduce((sum, s) => sum + s.confidence, 0) / suggestions.length;
      stats.averageConfidence = avgConfidence;
    }
  }, []);

  // 清理函数
  const cleanup = useCallback(() => {
    disposablesRef.current.forEach(d => d.dispose());
    disposablesRef.current.clear();

    engineRef.current = null;
    currentSuggestionsRef.current = [];
    lastContextRef.current = null;

    setCurrentSuggestions([]);
    setLastContext(null);
  }, []);

  // 计算统计信息
  const statistics = {
    totalSuggestions: statisticsRef.current.totalSuggestions,
    cacheHits: statisticsRef.current.cacheHits,
    averageConfidence: statisticsRef.current.totalConfidence /
      (statisticsRef.current.totalSuggestions || 1),
  };

  return {
    engine: engineRef.current,
    currentSuggestions,
    lastContext,
    getContext,
    forceRefresh,
    clearCache,
    setCacheEnabled,
    statistics,
  };
}

/**
 * 补全质量监控 Hook
 */
export function useCompletionQualityMonitor(
  suggestions: CompletionSuggestion[],
  onQualityUpdate?: (quality: CompletionQuality) => void
) {
  const [quality, setQuality] = useState<CompletionQuality>({
    averageConfidence: 0,
    highConfidenceCount: 0,
    lowConfidenceCount: 0,
    diversityScore: 0,
    relevanceScore: 0,
  });

  useEffect(() => {
    if (suggestions.length === 0) {
      return;
    }

    // 计算平均置信度
    const avgConfidence = suggestions.reduce((sum, s) => sum + s.confidence, 0) / suggestions.length;

    // 计算高低置信度数量
    const highConfidenceCount = suggestions.filter(s => s.confidence >= 0.8).length;
    const lowConfidenceCount = suggestions.filter(s => s.confidence < 0.5).length;

    // 计算多样性得分（基于建议的类型分布）
    const kindCounts = new Map<string, number>();
    suggestions.forEach(s => {
      kindCounts.set(s.kind, (kindCounts.get(s.kind) || 0) + 1);
    });
    const diversityScore = kindCounts.size / suggestions.length;

    // 计算相关性得分（基于源类型分布）
    const sourceCounts = new Map<string, number>();
    suggestions.forEach(s => {
      sourceCounts.set(s.source, (sourceCounts.get(s.source) || 0) + 1);
    });
    const relevanceScore = (sourceCounts.get('ai') || 0) / suggestions.length;

    const newQuality: CompletionQuality = {
      averageConfidence: avgConfidence,
      highConfidenceCount,
      lowConfidenceCount,
      diversityScore,
      relevanceScore,
    };

    setQuality(newQuality);

    if (onQualityUpdate) {
      onQualityUpdate(newQuality);
    }
  }, [suggestions, onQualityUpdate]);

  return quality;
}

/**
 * 补全质量指标
 */
export interface CompletionQuality {
  averageConfidence: number;
  highConfidenceCount: number;
  lowConfidenceCount: number;
  diversityScore: number;
  relevanceScore: number;
}

/**
 * 补全使用分析 Hook
 */
export function useCompletionUsageAnalytics() {
  const [analytics, setAnalytics] = useState<CompletionAnalytics>({
    totalCompletions: 0,
    acceptedCompletions: 0,
    rejectedCompletions: 0,
    averageTimeToAccept: 0,
    mostUsedKinds: new Map(),
    userPreferencePatterns: [],
  });

  const trackCompletion = useCallback((
    suggestions: CompletionSuggestion[],
    acceptedIndex?: number
  ) => {
    setAnalytics(prev => {
      const newAnalytics = { ...prev };

      newAnalytics.totalCompletions++;

      if (acceptedIndex !== undefined && acceptedIndex >= 0) {
        newAnalytics.acceptedCompletions++;
        const acceptedSuggestion = suggestions[acceptedIndex];

        // 跟踪最常用的类型
        const kindCounts = new Map(newAnalytics.mostUsedKinds);
        kindCounts.set(
          acceptedSuggestion.kind,
          (kindCounts.get(acceptedSuggestion.kind) || 0) + 1
        );
        newAnalytics.mostUsedKinds = kindCounts;

        // 分析用户偏好模式
        newAnalytics.userPreferencePatterns = analyzeUserPatterns(
          suggestions,
          acceptedIndex,
          newAnalytics.userPreferencePatterns
        );
      } else {
        newAnalytics.rejectedCompletions++;
      }

      return newAnalytics;
    });
  }, []);

  const getAcceptanceRate = useCallback(() => {
    if (analytics.totalCompletions === 0) return 0;
    return analytics.acceptedCompletions / analytics.totalCompletions;
  }, [analytics]);

  const getTopUsedKinds = useCallback((limit: number = 5) => {
    return Array.from(analytics.mostUsedKinds.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, limit);
  }, [analytics.mostUsedKinds]);

  return {
    analytics,
    trackCompletion,
    getAcceptanceRate,
    getTopUsedKinds,
  };
}

/**
 * 补全使用分析数据
 */
interface CompletionAnalytics {
  totalCompletions: number;
  acceptedCompletions: number;
  rejectedCompletions: number;
  averageTimeToAccept: number;
  mostUsedKinds: Map<string, number>;
  userPreferencePatterns: UserPreferencePattern[];
}

/**
 * 用户偏好模式
 */
interface UserPreferencePattern {
  pattern: string;
  frequency: number;
  confidence: number;
}

/**
 * 分析用户偏好模式
 */
function analyzeUserPatterns(
  suggestions: CompletionSuggestion[],
  acceptedIndex: number,
  existingPatterns: UserPreferencePattern[]
): UserPreferencePattern[] {
  const accepted = suggestions[acceptedIndex];
  const patterns = [...existingPatterns];

  // 分析位置偏好
  if (acceptedIndex < suggestions.length * 0.3) {
    addOrUpdatePattern(patterns, "prefers_first_suggestions", 0.8);
  } else if (acceptedIndex > suggestions.length * 0.7) {
    addOrUpdatePattern(patterns, "prefers_last_suggestions", 0.6);
  }

  // 分析源类型偏好
  if (accepted.source === 'ai') {
    addOrUpdatePattern(patterns, "prefers_ai_suggestions", 0.7);
  } else if (accepted.source === 'local') {
    addOrUpdatePattern(patterns, "prefers_local_suggestions", 0.5);
  }

  // 分析置信度偏好
  if (accepted.confidence >= 0.8) {
    addOrUpdatePattern(patterns, "prefers_high_confidence", 0.9);
  } else if (accepted.confidence >= 0.5) {
    addOrUpdatePattern(patterns, "accepts_medium_confidence", 0.6);
  }

  return patterns.slice(0, 10); // 保留最多10个模式
}

/**
 * 添加或更新模式
 */
function addOrUpdatePattern(
  patterns: UserPreferencePattern[],
  pattern: string,
  confidence: number
): void {
  const existing = patterns.find(p => p.pattern === pattern);
  if (existing) {
    existing.frequency++;
    existing.confidence = Math.min(existing.confidence + 0.1, 1.0);
  } else {
    patterns.push({
      pattern,
      frequency: 1,
      confidence,
    });
  }
}

/**
 * 实时补全建议展示 Hook
 */
export function useRealtimeSuggestions(
  editor: editor.IStandaloneCodeEditor | null,
  engine: IntelligentCompletionEngine | null,
  debounceMs: number = 300
) {
  const [suggestions, setSuggestions] = useState<CompletionSuggestion[]>([]);
  const [loading, setLoading] = useState(false);
  const debounceTimerRef = useRef<number | null>(null);

  const updateSuggestions = useCallback(async () => {
    if (!editor || !engine) {
      return;
    }

    const model = editor.getModel();
    if (!model) {
      return;
    }

    const position = editor.getPosition();
    if (!position) {
      return;
    }

    setLoading(true);

    try {
      const file = model.uri.fsPath || model.uri.path;
      const surroundingCode = model.getValue();

      const newSuggestions = await engine.getSuggestions(
        file,
        {
          lineNumber: position.lineNumber,
          column: position.column,
        },
        surroundingCode,
        {
          language: model.getLanguageId(),
          maxSuggestions: 10,
        }
      );

      setSuggestions(newSuggestions);
    } catch (error) {
      console.error('获取实时建议失败:', error);
      setSuggestions([]);
    } finally {
      setLoading(false);
    }
  }, [editor, engine]);

  // 监听光标位置变化
  useEffect(() => {
    if (!editor) {
      return;
    }

    const disposable = editor.onDidChangeCursorPosition(() => {
      // 防抖处理
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
      }

      debounceTimerRef.current = window.setTimeout(() => {
        updateSuggestions();
      }, debounceMs);
    });

    return () => {
      disposable.dispose();
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, [editor, updateSuggestions, debounceMs]);

  return {
    suggestions,
    loading,
    refresh: updateSuggestions,
  };
}