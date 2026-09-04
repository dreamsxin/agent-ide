/**
 * 智能代码补全系统 - 受 Comate 启发
 * 实现上下文感知的代码补全，包含中文优化提示词
 */

/**
 * 代码上下文信息
 */
export interface CodeContext {
  file: string;
  position: Position;
  language: string;
  surroundingCode: string;
  projectStructure: ProjectStructure;
  recentEdits: RecentEdit[];
  functionContext?: FunctionContext;
  variableContext?: VariableContext[];
  complexity: "low" | "medium" | "high";
}

/**
 * 位置信息
 */
export interface Position {
  lineNumber: number;
  column: number;
}

/**
 * 项目结构信息
 */
export interface ProjectStructure {
  summary: string;
  importantFiles: string[];
  dependencies: string[];
  frameworks: string[];
  patterns: CodePattern[];
}

/**
 * 代码模式
 */
export interface CodePattern {
  name: string;
  pattern: string;
  frequency: number;
  examples: string[];
}

/**
 * 最近编辑
 */
export interface RecentEdit {
  file: string;
  lineNumber: number;
  content: string;
  timestamp: number;
}

/**
 * 函数上下文
 */
export interface FunctionContext {
  name: string;
  parameters: string[];
  returnType: string;
  body: string;
  docstring?: string;
}

/**
 * 变量上下文
 */
export interface VariableContext {
  name: string;
  type: string;
  value?: string;
  scope: "local" | "function" | "global";
}

/**
 * 补全建议
 */
export interface CompletionSuggestion {
  label: string;
  kind: CompletionKind;
  detail?: string;
  documentation?: string;
  insertText: string;
  insertTextRules?: number;
  sortText: string;
  filterText?: string;
  range?: import("monaco-editor").IRange;
  confidence: number; // 0-1 置信度
  source: "local" | "ai" | "hybrid";
  reasoning?: string; // AI 生成的推理过程
}

/**
 * 补全类型
 */
export type CompletionKind =
  | "function"
  | "variable"
  | "class"
  | "interface"
  | "keyword"
  | "snippet"
  | "file"
  | "module"
  | "property"
  | "method";

/**
 * 上下文分析器
 */
export class ContextAnalyzer {
  private projectCache: Map<string, ProjectStructure> = new Map();
  private editHistory: RecentEdit[] = [];
  private maxHistorySize: number = 50;

  /**
   * 分析代码上下文
   */
  async analyze(context: {
    file: string;
    position: Position;
    surroundingCode: string;
    projectStructure?: ProjectStructure;
    recentEdits?: RecentEdit[];
    language?: string;
  }): Promise<CodeContext> {
    const {
      file,
      position,
      surroundingCode,
      projectStructure,
      recentEdits,
      language,
    } = context;

    // 获取或构建项目结构
    const structure = projectStructure || await this.buildProjectStructure(file);

    // 分析函数上下文
    const functionContext = this.analyzeFunctionContext(surroundingCode, position);

    // 分析变量上下文
    const variableContext = this.analyzeVariableContext(surroundingCode, position);

    // 评估复杂度
    const complexity = this.assessComplexity(surroundingCode, position);

    // 更新编辑历史
    this.updateEditHistory({
      file,
      lineNumber: position.lineNumber,
      content: surroundingCode,
      timestamp: Date.now(),
    });

    return {
      file,
      position,
      language: language || this.detectLanguage(file),
      surroundingCode,
      projectStructure: structure,
      recentEdits: recentEdits || this.getRecentEdits(file),
      functionContext,
      variableContext,
      complexity,
    };
  }

  /**
   * 构建项目结构
   */
  private async buildProjectStructure(file: string): Promise<ProjectStructure> {
    // 检查缓存
    if (this.projectCache.has(file)) {
      return this.projectCache.get(file)!;
    }

    // 这里应该实际分析项目文件
    // 简化实现：返回基本结构
    const structure: ProjectStructure = {
      summary: this.generateProjectSummary(file),
      importantFiles: [],
      dependencies: [],
      frameworks: [],
      patterns: [],
    };

    this.projectCache.set(file, structure);
    return structure;
  }

  /**
   * 生成项目摘要
   */
  private generateProjectSummary(file: string): string {
    // 简化实现：基于文件路径生成摘要
    const parts = file.split(/[/\\]/);
    const fileName = parts[parts.length - 1];
    const ext = fileName.split('.').pop() || '';

    return `项目文件: ${fileName}, 类型: ${ext}`;
  }

  /**
   * 分析函数上下文
   */
  private analyzeFunctionContext(code: string, position: Position): FunctionContext | undefined {
    // 简化实现：提取当前位置所在的函数
    const lines = code.split('\n');
    const currentLine = lines[position.lineNumber - 1] || '';

    // 简单的函数检测
    const functionMatch = currentLine.match(
      /(?:function|const|let|var)\s+(\w+)\s*(?:\([^)]*\))?\s*(?::\s*(\w+))?/
    );

    if (functionMatch) {
      return {
        name: functionMatch[1],
        parameters: [],
        returnType: functionMatch[2] || 'void',
        body: '',
      };
    }

    return undefined;
  }

  /**
   * 分析变量上下文
   */
  private analyzeVariableContext(code: string, position: Position): VariableContext[] {
    const variables: VariableContext[] = [];
    const lines = code.split('\n');

    // 分析当前位置之前的所有行，提取变量定义
    for (let i = 0; i < position.lineNumber - 1; i++) {
      const line = lines[i];
      const varMatches = line.matchAll(
        /(?:const|let|var)\s+(\w+)\s*(?::\s*(\w+))?\s*=\s*(.+?)(?:[;,]|$)/g
      );

      for (const match of varMatches) {
        variables.push({
          name: match[1],
          type: match[2] || 'any',
          value: match[3]?.trim(),
          scope: 'local',
        });
      }
    }

    return variables;
  }

  /**
   * 评估复杂度
   */
  private assessComplexity(code: string, position: Position): "low" | "medium" | "high" {
    const lines = code.split('\n');
    const currentLine = lines[position.lineNumber - 1] || '';

    // 简单的复杂度评估
    let score = 0;

    // 基于缩进深度
    const indentMatch = currentLine.match(/^(\s*)/);
    const indentDepth = indentMatch ? indentMatch[1].length : 0;
    score += Math.min(indentDepth / 2, 3);

    // 基于代码行数
    score += Math.min(lines.length / 50, 2);

    // 基于特殊字符
    const specialChars = (currentLine.match(/[{}()[\]]/g) || []).length;
    score += specialChars * 0.5;

    if (score < 2) return "low";
    if (score < 4) return "medium";
    return "high";
  }

  /**
   * 检测语言
   */
  private detectLanguage(file: string): string {
    const ext = file.split('.').pop() || '';
    const languageMap: Record<string, string> = {
      ts: 'typescript',
      tsx: 'typescript',
      js: 'javascript',
      jsx: 'javascript',
      py: 'python',
      rs: 'rust',
      go: 'go',
      java: 'java',
      cpp: 'cpp',
      c: 'c',
    };

    return languageMap[ext] || 'plaintext';
  }

  /**
   * 更新编辑历史
   */
  private updateEditHistory(edit: RecentEdit): void {
    this.editHistory.unshift(edit);
    if (this.editHistory.length > this.maxHistorySize) {
      this.editHistory = this.editHistory.slice(0, this.maxHistorySize);
    }
  }

  /**
   * 获取最近的编辑
   */
  private getRecentEdits(file: string): RecentEdit[] {
    return this.editHistory.filter(edit => edit.file === file);
  }

  /**
   * 清除缓存
   */
  clearCache(): void {
    this.projectCache.clear();
    this.editHistory = [];
  }
}

/**
 * 中文提示词优化器
 */
export class ChinesePromptOptimizer {
  /**
   * 优化代码补全提示词
   */
  optimizeCodeCompletion(context: CodeContext): string {
    const {
      file,
      position,
      language,
      surroundingCode,
      projectStructure,
      functionContext,
      variableContext,
      complexity,
    } = context;

    let prompt = `基于以下中文编程上下文提供智能代码补全建议：

## 基本信息
- 文件: ${file}
- 位置: 第 ${position.lineNumber} 行, 第 ${position.column} 列
- 语言: ${language}
- 复杂度: ${complexity}

## 当前代码
\`\`\`${language}
${surroundingCode}
\`\`\`
`;

    // 添加函数上下文
    if (functionContext) {
      prompt += `
## 函数上下文
- 函数名: ${functionContext.name}
- 返回类型: ${functionContext.returnType}
- 参数: ${functionContext.parameters.join(', ') || '无'}
`;
    }

    // 添加变量上下文
    if (variableContext && variableContext.length > 0) {
      prompt += `
## 可用变量
${variableContext.map(v => `- ${v.name}: ${v.type}${v.value ? ` = ${v.value}` : ''}`).join('\n')}
`;
    }

    // 添加项目上下文
    if (projectStructure.summary) {
      prompt += `
## 项目结构
${projectStructure.summary}
`;
    }

    prompt += `
## 补全要求
请提供符合以下要求的补全建议：
1. **中文命名习惯**: 考虑使用中文拼音或英文，根据项目风格保持一致
2. **中文注释风格**: 为复杂逻辑提供中文注释
3. **中文开发者习惯**: 遵循中文开发者常用的编程模式
4. **代码质量**: 确保代码符合最佳实践和语言规范
5. **上下文相关**: 基于当前函数、变量和项目结构提供相关建议

## 输出格式
请以 JSON 格式输出补全建议：
\`\`\`json
{
  "suggestions": [
    {
      "label": "补全文本",
      "kind": "function|variable|class|snippet等",
      "detail": "详细说明",
      "insertText": "实际插入的代码",
      "confidence": 0.95,
      "reasoning": "推荐理由（中文）"
    }
  ]
}
\`\`\`
`;

    return prompt;
  }

  /**
   * 优化错误解释提示词
   */
  optimizeErrorMessage(error: {
    message: string;
    location?: Position;
    code?: string;
  }): string {
    const { message, location, code } = error;

    return `
## 错误分析请求

### 错误信息
${message}

${location ? `
### 错误位置
- 行: ${location.lineNumber}
- 列: ${location.column}
` : ''}

${code ? `
### 相关代码
\`\`\`
${code}
\`\`\`
` : ''}

## 要求
请用中文提供以下分析：
1. **错误原因**: 详细解释为什么会发生这个错误
2. **解决方案**: 提供具体的修复建议和代码示例
3. **预防措施**: 如何避免类似错误的再次发生
4. **最佳实践**: 相关的编程最佳实践建议

请用清晰、易懂的中文回答，适合中文开发者理解。
`;
  }

  /**
   * 优化代码重构提示词
   */
  optimizeRefactoringPrompt(context: {
    code: string;
    target: string; // 重构目标，如"性能优化"、"可读性改进"等
    language: string;
  }): string {
    const { code, target, language } = context;

    return `
## 代码重构请求

### 重构目标
${target}

### 当前代码
\`\`\`${language}
${code}
\`\`\`

## 重构要求
请提供符合以下要求的重构建议：
1. **中文注释**: 为重构后的代码提供清晰的中文注释
2. **代码质量**: 提高代码的可读性、可维护性和性能
3. **最佳实践**: 遵循 ${language} 的最佳实践和设计模式
4. **向后兼容**: 确保重构不会破坏现有功能
5. **性能考虑**: 在适当的情况下优化性能

## 输出格式
请提供：
1. 重构后的代码（带中文注释）
2. 重构理由（中文）
3. 潜在风险和注意事项（中文）
4. 测试建议（中文）
`;
  }
}

/**
 * 智能补全引擎
 */
export class IntelligentCompletionEngine {
  private contextAnalyzer: ContextAnalyzer;
  private promptOptimizer: ChinesePromptOptimizer;
  private suggestionCache: Map<string, CompletionSuggestion[]> = new Map();
  private cacheEnabled: boolean = true;
  private maxCacheSize: number = 100;

  constructor() {
    this.contextAnalyzer = new ContextAnalyzer();
    this.promptOptimizer = new ChinesePromptOptimizer();
  }

  /**
   * 获取补全建议
   */
  async getSuggestions(
    file: string,
    position: Position,
    surroundingCode: string,
    options: {
      language?: string;
      projectStructure?: ProjectStructure;
      maxSuggestions?: number;
    } = {}
  ): Promise<CompletionSuggestion[]> {
    const {
      language,
      projectStructure,
      maxSuggestions = 10,
    } = options;

    // 检查缓存
    const cacheKey = this.generateCacheKey(file, position, surroundingCode);
    if (this.cacheEnabled && this.suggestionCache.has(cacheKey)) {
      return this.suggestionCache.get(cacheKey)!.slice(0, maxSuggestions);
    }

    // 分析上下文
    const context = await this.contextAnalyzer.analyze({
      file,
      position,
      surroundingCode,
      projectStructure,
      language,
    });

    // 生成优化后的提示词
    // 这里应该调用 AI 模型生成补全建议
    // 简化实现：生成基础建议
    const suggestions = await this.generateSuggestions(context, maxSuggestions);

    // 缓存结果
    if (this.cacheEnabled) {
      this.cacheSuggestions(cacheKey, suggestions);
    }

    return suggestions.slice(0, maxSuggestions);
  }

  /**
   * 生成补全建议
   */
  private async generateSuggestions(
    context: CodeContext,
    maxSuggestions: number
  ): Promise<CompletionSuggestion[]> {
    // 这里应该调用实际的 AI 模型
    // 简化实现：基于上下文生成基础建议

    const suggestions: CompletionSuggestion[] = [];

    // 基于变量上下文生成建议
    if (context.variableContext && context.variableContext.length > 0) {
      for (const variable of context.variableContext) {
        suggestions.push({
          label: variable.name,
          kind: this.mapToCompletionKind(variable.type),
          detail: `${variable.type} 变量`,
          insertText: variable.name,
          sortText: `1-${variable.name}`,
          confidence: 0.8,
          source: "local",
          reasoning: `基于当前作用域内的变量定义`,
        });
      }
    }

    // 基于函数上下文生成建议
    if (context.functionContext) {
      suggestions.push({
        label: context.functionContext.name,
        kind: "function",
        detail: `${context.functionContext.returnType} 函数`,
        insertText: context.functionContext.name,
        sortText: `2-${context.functionContext.name}`,
        confidence: 0.9,
        source: "local",
        reasoning: `基于当前函数定义`,
      });
    }

    // 基于语言关键字生成建议
    const languageKeywords = this.getLanguageKeywords(context.language);
    for (const keyword of languageKeywords) {
      suggestions.push({
        label: keyword,
        kind: "keyword",
        insertText: keyword,
        sortText: `3-${keyword}`,
        confidence: 0.7,
        source: "local",
        reasoning: `${context.language} 语言关键字`,
      });
    }

    // 按置信度排序
    suggestions.sort((a, b) => b.confidence - a.confidence);

    return suggestions.slice(0, Math.max(0, maxSuggestions));
  }

  /**
   * 获取语言关键字
   */
  private getLanguageKeywords(language: string): string[] {
    const keywordMap: Record<string, string[]> = {
      typescript: ['const', 'let', 'var', 'function', 'class', 'interface', 'type', 'import', 'export'],
      javascript: ['const', 'let', 'var', 'function', 'class', 'import', 'export'],
      python: ['def', 'class', 'import', 'from', 'if', 'else', 'for', 'while', 'try', 'except'],
      rust: ['fn', 'struct', 'enum', 'impl', 'use', 'mod', 'let', 'mut', 'pub'],
      go: ['func', 'type', 'struct', 'interface', 'package', 'import', 'var', 'const'],
    };

    return keywordMap[language] || [];
  }

  /**
   * 映射到补全类型
   */
  private mapToCompletionKind(type: string): CompletionKind {
    const typeMap: Record<string, CompletionKind> = {
      'function': 'function',
      'class': 'class',
      'interface': 'interface',
      'string': 'variable',
      'number': 'variable',
      'boolean': 'variable',
      'any': 'variable',
    };

    return typeMap[type] || 'variable';
  }

  /**
   * 生成缓存键
   */
  private generateCacheKey(
    file: string,
    position: Position,
    surroundingCode: string
  ): string {
    // 使用位置和代码的哈希作为缓存键
    return `${file}:${position.lineNumber}:${position.column}:${this.hashString(surroundingCode)}`;
  }

  /**
   * 简单的字符串哈希
   */
  private hashString(str: string): string {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      const char = str.charCodeAt(i);
      hash = ((hash << 5) - hash) + char;
      hash = hash & hash; // Convert to 32bit integer
    }
    return Math.abs(hash).toString(36);
  }

  /**
   * 缓存建议
   */
  private cacheSuggestions(key: string, suggestions: CompletionSuggestion[]): void {
    // 清理旧缓存
    if (this.suggestionCache.size >= this.maxCacheSize) {
      const keys = Array.from(this.suggestionCache.keys());
      const removeCount = this.suggestionCache.size - this.maxCacheSize + 1;
      for (let i = 0; i < removeCount; i++) {
        this.suggestionCache.delete(keys[i]);
      }
    }

    this.suggestionCache.set(key, suggestions);
  }

  /**
   * 清除缓存
   */
  clearCache(): void {
    this.suggestionCache.clear();
  }

  /**
   * 启用/禁用缓存
   */
  setCacheEnabled(enabled: boolean): void {
    this.cacheEnabled = enabled;
  }

  /**
   * 获取上下文分析器
   */
  getContextAnalyzer(): ContextAnalyzer {
    return this.contextAnalyzer;
  }

  /**
   * 获取提示词优化器
   */
  getPromptOptimizer(): ChinesePromptOptimizer {
    return this.promptOptimizer;
  }
}

/**
 * 创建智能补全引擎实例
 */
export function createIntelligentCompletionEngine(): IntelligentCompletionEngine {
  return new IntelligentCompletionEngine();
}

/**
 * Monaco 补全提供者适配器
 */
export function createMonacoCompletionProvider(
  engine: IntelligentCompletionEngine,
  monaco: typeof import("monaco-editor")
): import("monaco-editor").languages.CompletionItemProvider {
  return {
    triggerCharacters: ['.', '/', '\\', '"', "'", '@', '<', ' ', '(', '[', '{'],
    provideCompletionItems: async (model, position, _context, _token) => {
      const file = model.uri.fsPath || model.uri.path;
      const surroundingCode = model.getValue();
      const word = model.getWordUntilPosition(position);

      try {
        const suggestions = await engine.getSuggestions(file, {
          lineNumber: position.lineNumber,
          column: position.column,
        }, surroundingCode, {
          language: model.getLanguageId(),
          maxSuggestions: 20,
        });

        return {
          suggestions: suggestions.map(suggestion => ({
            label: suggestion.label,
            kind: mapToMonacoCompletionKind(monaco, suggestion.kind),
            detail: suggestion.detail,
            documentation: suggestion.documentation ? { value: suggestion.documentation } : undefined,
            insertText: suggestion.insertText,
            insertTextRules: suggestion.insertTextRules,
            sortText: suggestion.sortText,
            filterText: suggestion.filterText,
            range: {
              startLineNumber: position.lineNumber,
              endLineNumber: position.lineNumber,
              startColumn: word.startColumn,
              endColumn: word.endColumn,
            },
            // 添加自定义属性
            confidence: suggestion.confidence,
            source: suggestion.source,
            reasoning: suggestion.reasoning,
          })),
        };
      } catch (error) {
        console.error('智能补全生成失败:', error);
        return { suggestions: [] };
      }
    },
  };

  // 辅助方法
  function mapToMonacoCompletionKind(
    monaco: typeof import("monaco-editor"),
    kind: CompletionKind
  ): import("monaco-editor").languages.CompletionItemKind {
    const kindMap: Record<CompletionKind, import("monaco-editor").languages.CompletionItemKind> = {
      'function': monaco.languages.CompletionItemKind.Function,
      'variable': monaco.languages.CompletionItemKind.Variable,
      'class': monaco.languages.CompletionItemKind.Class,
      'interface': monaco.languages.CompletionItemKind.Interface,
      'keyword': monaco.languages.CompletionItemKind.Keyword,
      'snippet': monaco.languages.CompletionItemKind.Snippet,
      'file': monaco.languages.CompletionItemKind.File,
      'module': monaco.languages.CompletionItemKind.Module,
      'property': monaco.languages.CompletionItemKind.Property,
      'method': monaco.languages.CompletionItemKind.Method,
    };

    return kindMap[kind] || monaco.languages.CompletionItemKind.Text;
  }
}