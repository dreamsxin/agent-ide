/**
 * 混合模型策略 - 智能模型选择和任务分配
 * 根据任务复杂度、成本预算、性能要求自动选择最佳模型
 */


/**
 * 任务类型
 */
export enum TaskType {
  SimpleCompletion = "simple_completion",      // 简单补全
  CodeGeneration = "code_generation",          // 代码生成
  ComplexRefactoring = "complex_refactoring",  // 复杂重构
  BugFix = "bug_fix",                          // Bug 修复
  Documentation = "documentation",              // 文档生成
  Testing = "testing",                          // 测试生成
  CodeReview = "code_review",                  // 代码审查
  Optimization = "optimization",               // 性能优化
  OfflineTask = "offline_task",                // 离线任务
}

/**
 * 任务复杂度
 */
export enum ComplexityLevel {
  Low = "low",          // 低复杂度
  Medium = "medium",    // 中等复杂度
  High = "high",        // 高复杂度
}

/**
 * 模型源
 */
export enum ModelSource {
  Local = "local",      // 本地模型
  Cloud = "cloud",      // 云端模型
  Hybrid = "hybrid",    // 混合模式
}

/**
 * 模型选择结果
 */
export interface ModelSelection {
  source: ModelSource;
  modelName: string;
  modelType: ModelType;
  capabilities: ModelCapabilities;
  reasoning: string;
  estimatedCost: number;
  estimatedLatency: number;
  confidence: number;
}

/**
 * 任务上下文
 */
export interface TaskContext {
  taskType: TaskType;
  complexity: ComplexityLevel;
  language?: string;
  codeLength?: number;
  expectedOutputLength?: number;
  timeConstraint?: number; // 毫秒
  costConstraint?: number;
  offlineRequired?: boolean;
  privacyRequired?: boolean;
}

/**
 * 成本估算
 */
export interface CostEstimate {
  estimatedTokens: number;
  estimatedCost: number; // 美元
  estimatedLatency: number; // 毫秒
  costPerToken: number;
  memoryUsage: number; // MB
}

/**
 * 模型选择器配置
 */
export interface ModelSelectorConfig {
  preferLocalForSimpleTasks: boolean;
  maxLocalLatency: number;
  costThreshold: number;
  privacyMode: boolean;
  offlineMode: boolean;
  fallbackToCloud: boolean;
  cacheEnabled: boolean;
}

/**
 * 模型选择器
 */
export class ModelSelector {
  private config: ModelSelectorConfig;
  private availableModels: Map<string, ModelInfo>;
  private costTracker: CostTracker;

  constructor(config: Partial<ModelSelectorConfig> = {}) {
    this.config = {
      preferLocalForSimpleTasks: config.preferLocalForSimpleTasks ?? true,
      maxLocalLatency: config.maxLocalLatency ?? 500, // 500ms
      costThreshold: config.costThreshold ?? 0.01, // $0.01
      privacyMode: config.privacyMode ?? false,
      offlineMode: config.offlineMode ?? false,
      fallbackToCloud: config.fallbackToCloud ?? true,
      cacheEnabled: config.cacheEnabled ?? true,
    };

    this.availableModels = new Map();
    this.costTracker = new CostTracker();

    this.initializeDefaultModels();
  }

  /**
   * 初始化默认模型
   */
  private initializeDefaultModels(): void {
    // 本地模型
    this.addModel({
      name: "starcoder",
      source: ModelSource.Local,
      type: "starcoder",
      capabilities: {
        maxContextTokens: 8192,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java"],
        inferenceSpeedMs: 80,
        memoryRequirementMb: 4096,
        supportsStreaming: true,
        supportsToolCalls: false,
        costPerToken: 0.0, // 本地模型免费
      },
      enabled: true,
    });

    this.addModel({
      name: "codellama",
      source: ModelSource.Local,
      type: "codellama",
      capabilities: {
        maxContextTokens: 16384,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java", "cpp", "c"],
        inferenceSpeedMs: 60,
        memoryRequirementMb: 8192,
        supportsStreaming: true,
        supportsToolCalls: false,
        costPerToken: 0.0,
      },
      enabled: false, // 默认禁用，需要用户启用
    });

    this.addModel({
      name: "deepseek-coder",
      source: ModelSource.Local,
      type: "deepseek-coder",
      capabilities: {
        maxContextTokens: 4096,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java"],
        inferenceSpeedMs: 50,
        memoryRequirementMb: 2048,
        supportsStreaming: true,
        supportsToolCalls: false,
        costPerToken: 0.0,
      },
      enabled: false,
    });

    // 云端模型
    this.addModel({
      name: "gpt-4",
      source: ModelSource.Cloud,
      type: "openai",
      capabilities: {
        maxContextTokens: 128000,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java", "cpp", "c", "ruby", "php"],
        inferenceSpeedMs: 20,
        memoryRequirementMb: 0,
        supportsStreaming: true,
        supportsToolCalls: true,
        costPerToken: 0.00003, // $0.03 per 1K tokens
      },
      enabled: true,
    });

    this.addModel({
      name: "gpt-4o-mini",
      source: ModelSource.Cloud,
      type: "openai",
      capabilities: {
        maxContextTokens: 128000,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java", "cpp", "c"],
        inferenceSpeedMs: 15,
        memoryRequirementMb: 0,
        supportsStreaming: true,
        supportsToolCalls: true,
        costPerToken: 0.0000015, // $0.0015 per 1K tokens
      },
      enabled: true,
    });

    this.addModel({
      name: "deepseek-chat",
      source: ModelSource.Cloud,
      type: "deepseek",
      capabilities: {
        maxContextTokens: 128000,
        supportedLanguages: ["typescript", "javascript", "python", "rust", "go", "java", "cpp", "c"],
        inferenceSpeedMs: 25,
        memoryRequirementMb: 0,
        supportsStreaming: true,
        supportsToolCalls: true,
        costPerToken: 0.0000014, // DeepSeek 价格更低
      },
      enabled: true,
    });
  }

  /**
   * 添加模型
   */
  addModel(model: ModelInfo): void {
    this.availableModels.set(model.name, model);
  }

  /**
   * 移除模型
   */
  removeModel(name: string): void {
    this.availableModels.delete(name);
  }

  /**
   * 启用/禁用模型
   */
  setModelEnabled(name: string, enabled: boolean): void {
    const model = this.availableModels.get(name);
    if (model) {
      model.enabled = enabled;
    }
  }

  /**
   * 为任务选择最佳模型
   */
  selectBestModel(context: TaskContext): ModelSelection {
    // 1. 检查强制约束
    if (context.offlineRequired || this.config.offlineMode) {
      return this.selectBestLocalModel(context);
    }

    if (context.privacyRequired || this.config.privacyMode) {
      const localModels = this.getEnabledModelsBySource(ModelSource.Local);
      if (localModels.length > 0) {
        return this.selectBestLocalModel(context);
      }
    }

    // 2. 根据任务类型和复杂度选择
    switch (context.complexity) {
      case ComplexityLevel.Low:
        return this.selectForLowComplexity(context);
      case ComplexityLevel.Medium:
        return this.selectForMediumComplexity(context);
      case ComplexityLevel.High:
        return this.selectForHighComplexity(context);
    }
  }

  /**
   * 为低复杂度任务选择模型
   */
  private selectForLowComplexity(context: TaskContext): ModelSelection {
    // 优先使用本地模型
    if (this.config.preferLocalForSimpleTasks) {
      const localModel = this.selectFastestLocalModel(context);
      if (localModel) {
        return localModel;
      }
    }

    // 备选：使用最便宜的云端模型
    return this.selectCheapestCloudModel(context);
  }

  /**
   * 为中等复杂度任务选择模型
   */
  private selectForMediumComplexity(context: TaskContext): ModelSelection {
    // 平衡策略：在本地和云端之间选择
    const localModels = this.getEnabledModelsBySource(ModelSource.Local);
    const cloudModels = this.getEnabledModelsBySource(ModelSource.Cloud);

    if (localModels.length === 0) {
      return this.selectBestCloudModel(context);
    }

    if (cloudModels.length === 0) {
      return this.selectBestLocalModel(context);
    }

    // 比较性能和成本
    const bestLocal = this.selectBestLocalModel(context);
    const bestCloud = this.selectBestCloudModel(context);

    // 根据时间约束选择
    if (context.timeConstraint && context.timeConstraint < this.config.maxLocalLatency) {
      return bestCloud;
    }

    // 根据成本约束选择
    if (context.costConstraint && context.costConstraint < bestCloud.estimatedCost) {
      return bestLocal;
    }

    // 默认选择本地模型
    return bestLocal;
  }

  /**
   * 为高复杂度任务选择模型
   */
  private selectForHighComplexity(context: TaskContext): ModelSelection {
    // 高复杂度任务优先使用能力最强的云端模型
    const cloudModel = this.selectMostCapableCloudModel(context);

    // 检查是否有回退选项
    if (this.config.fallbackToCloud || !cloudModel) {
      return cloudModel || this.selectBestCloudModel(context);
    }

    // 如果不能使用云端，尝试本地模型
    return this.selectBestLocalModel(context);
  }

  /**
   * 选择最佳本地模型
   */
  private selectBestLocalModel(context: TaskContext): ModelSelection {
    const localModels = this.getEnabledModelsBySource(ModelSource.Local);

    if (localModels.length === 0) {
      // 没有本地模型可用，回退到云端
      if (this.config.fallbackToCloud) {
        return this.selectBestCloudModel(context);
      }
      throw new Error("No local models available and cloud fallback disabled");
    }

    // 根据语言支持选择
    if (context.language) {
      const languageMatch = localModels.find(model =>
        model.capabilities.supportedLanguages.includes(context.language!)
      );
      if (languageMatch) {
        return this.createModelSelection(languageMatch, context);
      }
    }

    // 选择最快的本地模型
    const fastest = this.selectFastestLocalModel(context);
    if (!fastest) {
      throw new Error("No local models available");
    }
    return fastest;
  }

  /**
   * 选择最快的本地模型
   */
  private selectFastestLocalModel(context: TaskContext): ModelSelection | null {
    const localModels = this.getEnabledModelsBySource(ModelSource.Local);
    if (localModels.length === 0) return null;

    const fastest = localModels.reduce((best, current) =>
      current.capabilities.inferenceSpeedMs < best.capabilities.inferenceSpeedMs ? current : best
    );

    return this.createModelSelection(fastest, context);
  }

  /**
   * 选择最佳云端模型
   */
  private selectBestCloudModel(context: TaskContext): ModelSelection {
    const cloudModels = this.getEnabledModelsBySource(ModelSource.Cloud);
    if (cloudModels.length === 0) {
      throw new Error("No cloud models available");
    }

    // 根据语言支持选择
    if (context.language) {
      const languageMatch = cloudModels.find(model =>
        model.capabilities.supportedLanguages.includes(context.language!)
      );
      if (languageMatch) {
        return this.createModelSelection(languageMatch, context);
      }
    }

    // 默认选择第一个可用的云端模型
    return this.createModelSelection(cloudModels[0], context);
  }

  /**
   * 选择最便宜的云端模型
   */
  private selectCheapestCloudModel(context: TaskContext): ModelSelection {
    const cloudModels = this.getEnabledModelsBySource(ModelSource.Cloud);
    if (cloudModels.length === 0) {
      throw new Error("No cloud models available");
    }

    const cheapest = cloudModels.reduce((best, current) =>
      current.capabilities.costPerToken < best.capabilities.costPerToken ? current : best
    );

    return this.createModelSelection(cheapest, context);
  }

  /**
   * 选择能力最强的云端模型
   */
  private selectMostCapableCloudModel(context: TaskContext): ModelSelection | null {
    const cloudModels = this.getEnabledModelsBySource(ModelSource.Cloud);
    if (cloudModels.length === 0) return null;

    // 根据上下文长度选择
    const expectedTokens = this.estimateTokens(context);
    const capableModels = cloudModels.filter(model =>
      model.capabilities.maxContextTokens >= expectedTokens
    );

    if (capableModels.length === 0) {
      // 没有模型能满足上下文需求，选择容量最大的
      const largest = cloudModels.reduce((best, current) =>
        current.capabilities.maxContextTokens > best.capabilities.maxContextTokens ? current : best
      );
      return this.createModelSelection(largest, context);
    }

    // 在满足需求的模型中选择最便宜的
    const best = capableModels.reduce((best, current) =>
      current.capabilities.costPerToken < best.capabilities.costPerToken ? current : best
    );

    return this.createModelSelection(best, context);
  }

  /**
   * 创建模型选择结果
   */
  private createModelSelection(model: ModelInfo, context: TaskContext): ModelSelection {
    const estimate = this.estimateCost(model, context);

    const reasoning = this.generateReasoning(model, context, estimate);

    return {
      source: model.source,
      modelName: model.name,
      modelType: model.type,
      capabilities: model.capabilities,
      reasoning,
      estimatedCost: estimate.estimatedCost,
      estimatedLatency: estimate.estimatedLatency,
      confidence: this.calculateConfidence(model, context),
    };
  }

  /**
   * 生成选择理由
   */
  private generateReasoning(
    model: ModelInfo,
    context: TaskContext,
    estimate: CostEstimate
  ): string {
    const reasons: string[] = [];

    // 来源理由
    reasons.push(model.source === ModelSource.Local
      ? "选择本地模型以确保隐私和离线能力"
      : "选择云端模型以获得更强的推理能力");

    // 复杂度匹配
    if (context.complexity === ComplexityLevel.Low && model.source === ModelSource.Local) {
      reasons.push("低复杂度任务适合使用本地模型");
    } else if (context.complexity === ComplexityLevel.High && model.source === ModelSource.Cloud) {
      reasons.push("高复杂度任务需要云端模型的能力");
    }

    // 语言支持
    if (context.language && model.capabilities.supportedLanguages.includes(context.language)) {
      reasons.push(`模型支持目标语言: ${context.language}`);
    }

    // 成本考虑
    if (estimate.estimatedCost < this.config.costThreshold) {
      reasons.push(`预估成本符合预算: $${estimate.estimatedCost.toFixed(4)}`);
    }

    // 性能考虑
    if (context.timeConstraint && estimate.estimatedLatency <= context.timeConstraint) {
      reasons.push(`预估延迟符合时间约束: ${estimate.estimatedLatency}ms`);
    }

    return reasons.join("；");
  }

  /**
   * 计算置信度
   */
  private calculateConfidence(model: ModelInfo, context: TaskContext): number {
    let confidence = 0.5; // 基础置信度

    // 语言支持加分
    if (context.language && model.capabilities.supportedLanguages.includes(context.language)) {
      confidence += 0.2;
    }

    // 复杂度匹配加分
    if (context.complexity === ComplexityLevel.Low && model.source === ModelSource.Local) {
      confidence += 0.2;
    } else if (context.complexity === ComplexityLevel.High && model.source === ModelSource.Cloud) {
      confidence += 0.3;
    }

    // 约束满足加分
    if (context.costConstraint) {
      const estimate = this.estimateCost(model, context);
      if (estimate.estimatedCost <= context.costConstraint) {
        confidence += 0.1;
      }
    }

    return Math.min(confidence, 1.0);
  }

  /**
   * 估算 Token 数量
   */
  private estimateTokens(context: TaskContext): number {
    // 简化的 token 估算
    const baseTokens = 1000; // 基础 token
    const codeTokens = (context.codeLength || 0) / 4; // 假设 4 字符 = 1 token
    const outputTokens = context.expectedOutputLength || 500;

    return baseTokens + codeTokens + outputTokens;
  }

  /**
   * 估算成本和性能
   */
  estimateCost(model: ModelInfo, context: TaskContext): CostEstimate {
    const estimatedTokens = this.estimateTokens(context);

    const estimatedCost = estimatedTokens * model.capabilities.costPerToken;
    const estimatedLatency = model.capabilities.inferenceSpeedMs * estimatedTokens / 1000;

    return {
      estimatedTokens,
      estimatedCost,
      estimatedLatency,
      costPerToken: model.capabilities.costPerToken,
      memoryUsage: model.capabilities.memoryRequirementMb,
    };
  }

  /**
   * 获取启用模型列表
   */
  getEnabledModels(): ModelInfo[] {
    return Array.from(this.availableModels.values()).filter(model => model.enabled);
  }

  /** 获取模型信息 */
  getModel(name: string): ModelInfo | undefined {
    return this.availableModels.get(name);
  }

  /**
   * 根据源获取启用模型
   */
  getEnabledModelsBySource(source: ModelSource): ModelInfo[] {
    return this.getEnabledModels().filter(model => model.source === source);
  }

  /**
   * 获取成本追踪器
   */
  getCostTracker(): CostTracker {
    return this.costTracker;
  }

  /**
   * 更新配置
   */
  updateConfig(config: Partial<ModelSelectorConfig>): void {
    this.config = { ...this.config, ...config };
  }

  /**
   * 获取配置
   */
  getConfig(): ModelSelectorConfig {
    return { ...this.config };
  }

  /**
   * 重置为默认配置
   */
  resetConfig(): void {
    this.config = {
      preferLocalForSimpleTasks: true,
      maxLocalLatency: 500,
      costThreshold: 0.01,
      privacyMode: false,
      offlineMode: false,
      fallbackToCloud: true,
      cacheEnabled: true,
    };
  }
}

/**
 * 模型信息
 */
interface ModelInfo {
  name: string;
  source: ModelSource;
  type: ModelType;
  capabilities: ModelCapabilities;
  enabled: boolean;
}

/**
 * 成本追踪器
 */
export class CostTracker {
  private totalCost: number = 0;
  private totalTokens: number = 0;
  private modelUsage: Map<string, number> = new Map();
  private costHistory: CostRecord[] = [];

  /**
   * 记录成本
   */
  recordCost(modelName: string, tokens: number, cost: number): void {
    this.totalCost += cost;
    this.totalTokens += tokens;

    const currentUsage = this.modelUsage.get(modelName) || 0;
    this.modelUsage.set(modelName, currentUsage + cost);

    this.costHistory.push({
      timestamp: Date.now(),
      modelName,
      tokens,
      cost,
    });
  }

  /**
   * 获取总成本
   */
  getTotalCost(): number {
    return this.totalCost;
  }

  /**
   * 获取总 Token 数
   */
  getTotalTokens(): number {
    return this.totalTokens;
  }

  /**
   * 获取模型使用情况
   */
  getModelUsage(): Map<string, number> {
    return new Map(this.modelUsage);
  }

  /**
   * 获取成本历史
   */
  getCostHistory(): CostRecord[] {
    return [...this.costHistory];
  }

  /**
   * 重置追踪器
   */
  reset(): void {
    this.totalCost = 0;
    this.totalTokens = 0;
    this.modelUsage.clear();
    this.costHistory = [];
  }

  /**
   * 生成成本报告
   */
  generateReport(): string {
    const lines: string[] = [
      "=== 模型使用成本报告 ===",
      "",
      `总成本: $${this.totalCost.toFixed(4)}`,
      `总 Token 数: ${this.totalTokens.toLocaleString()}`,
      `平均成本 per Token: $${(this.totalCost / this.totalTokens).toFixed(6)}`,
      "",
      "模型使用分布:",
    ];

    for (const [modelName, cost] of this.modelUsage) {
      const percentage = (cost / this.totalCost * 100).toFixed(1);
      lines.push(`  ${modelName}: $${cost.toFixed(4)} (${percentage}%)`);
    }

    return lines.join("\n");
  }
}

/**
 * 成本记录
 */
interface CostRecord {
  timestamp: number;
  modelName: string;
  tokens: number;
  cost: number;
}

/**
 * 混合模型策略管理器
 */
export class HybridModelStrategy {
  private modelSelector: ModelSelector;
  private costTracker: CostTracker;

  constructor(config?: Partial<ModelSelectorConfig>) {
    this.modelSelector = new ModelSelector(config);
    this.costTracker = this.modelSelector.getCostTracker();
  }

  /**
   * 为任务选择模型并执行
   */
  async executeTask(
    task: () => Promise<string>,
    context: TaskContext
  ): Promise<{ result: string; modelSelection: ModelSelection }> {
    // 选择模型
    const modelSelection = this.modelSelector.selectBestModel(context);

    // 执行任务
    const result = await task();

    // 记录成本（简化实现）
    const selectedModel = this.modelSelector.getModel(modelSelection.modelName);
    if (!selectedModel) {
      throw new Error(`Model not available: ${modelSelection.modelName}`);
    }
    const costEstimate = this.modelSelector.estimateCost(selectedModel, context);

    this.costTracker.recordCost(
      modelSelection.modelName,
      costEstimate.estimatedTokens,
      costEstimate.estimatedCost
    );

    return { result, modelSelection };
  }

  /**
   * 获取模型选择器
   */
  getModelSelector(): ModelSelector {
    return this.modelSelector;
  }

  /**
   * 获取成本追踪器
   */
  getCostTracker(): CostTracker {
    return this.costTracker;
  }
}

// 辅助类型
export interface ModelCapabilities {
  maxContextTokens: number;
  supportedLanguages: string[];
  inferenceSpeedMs: number;
  memoryRequirementMb: number;
  supportsStreaming: boolean;
  supportsToolCalls: boolean;
  costPerToken: number;
}

export type ModelType = "starcoder" | "codellama" | "deepseek-coder" | "openai" | "deepseek";