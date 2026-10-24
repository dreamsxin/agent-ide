import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ContextAnalyzer,
  ChinesePromptOptimizer,
  IntelligentCompletionEngine,
  createIntelligentCompletionEngine,
  createMonacoCompletionProvider,
} from "./intelligentCompletion";

describe("ContextAnalyzer", () => {
  const analyzer = new ContextAnalyzer();

  it("detects language from file extension when not provided", async () => {
    const context = await analyzer.analyze({
      file: "src/main.rs",
      position: { lineNumber: 1, column: 1 },
      surroundingCode: "fn main() {}",
    });

    expect(context.language).toBe("rust");
  });

  it("extracts variables declared before the cursor line", async () => {
    const context = await analyzer.analyze({
      file: "src/app.ts",
      position: { lineNumber: 3, column: 1 },
      surroundingCode: ["const total = 42;", "let name: string = 'ide';", ""].join("\n"),
      language: "typescript",
    });

    expect(context.variableContext).toEqual([
      { name: "total", type: "any", value: "42", scope: "local" },
      { name: "name", type: "string", value: "'ide'", scope: "local" },
    ]);
  });

  it("detects the function containing the cursor line", async () => {
    const context = await analyzer.analyze({
      file: "src/app.ts",
      position: { lineNumber: 1, column: 10 },
      surroundingCode: "function computeTotal(items: string[]): number {",
      language: "typescript",
    });

    expect(context.functionContext?.name).toBe("computeTotal");
    expect(context.functionContext?.returnType).toBe("number");
  });

  it("keeps per-file edit history and caps it", async () => {
    const analyzer = new ContextAnalyzer();
    for (let i = 0; i < 60; i++) {
      await analyzer.analyze({
        file: "src/app.ts",
        position: { lineNumber: 1, column: 1 },
        surroundingCode: `line ${i}`,
      });
    }
    await analyzer.analyze({
      file: "src/other.ts",
      position: { lineNumber: 1, column: 1 },
      surroundingCode: "other",
    });

    const context = await analyzer.analyze({
      file: "src/app.ts",
      position: { lineNumber: 1, column: 1 },
      surroundingCode: "line again",
    });

    // The global 50-entry history holds 49 app entries after this call
    // because the interleaved other-file edit displaced the oldest one.
    expect(context.recentEdits?.length).toBe(49);
    expect(context.recentEdits?.every((edit) => edit.file === "src/app.ts")).toBe(true);
  });
});

describe("IntelligentCompletionEngine", () => {
  let engine: IntelligentCompletionEngine;

  beforeEach(() => {
    engine = createIntelligentCompletionEngine();
  });

  it("suggests scope variables, current function, and language keywords", async () => {
    // The analyzer inspects the cursor line for the enclosing function,
    // so the declaration sits on line 3 with variables declared above.
    const suggestions = await engine.getSuggestions(
      "src/app.ts",
      { lineNumber: 3, column: 1 },
      ["const total = 42;", "let name: string = 'ide';", "function compute() {"].join("\n"),
      { language: "typescript", maxSuggestions: 20 }
    );

    const labels = suggestions.map((s) => s.label);
    expect(labels).toContain("total");
    expect(labels).toContain("compute");
    expect(labels).toContain("const");

    // sorted by descending confidence
    const confidences = suggestions.map((s) => s.confidence);
    expect([...confidences].sort((a, b) => b - a)).toEqual(confidences);
  });

  it("clamps results to maxSuggestions", async () => {
    const suggestions = await engine.getSuggestions(
      "src/app.ts",
      { lineNumber: 1, column: 1 },
      "const value = 1;",
      { language: "typescript", maxSuggestions: 2 }
    );

    expect(suggestions).toHaveLength(2);
  });

  it("serves identical results from cache and bypasses cache when disabled", async () => {
    const code = "const cached = true;";
    const first = await engine.getSuggestions(
      "src/cache.ts",
      { lineNumber: 1, column: 1 },
      code,
      { language: "typescript", maxSuggestions: 5 }
    );
    const second = await engine.getSuggestions(
      "src/cache.ts",
      { lineNumber: 1, column: 1 },
      code,
      { language: "typescript", maxSuggestions: 5 }
    );
    expect(second).toEqual(first);

    engine.setCacheEnabled(false);
    const uncached = await engine.getSuggestions(
      "src/cache.ts",
      { lineNumber: 1, column: 1 },
      code,
      { language: "typescript", maxSuggestions: 5 }
    );
    expect(uncached.map((s) => s.label)).toEqual(first.map((s) => s.label));

    engine.clearCache();
  });

  it("returns keyword suggestions for known languages and none for unknown ones", async () => {
    const goSuggestions = await engine.getSuggestions(
      "main.go",
      { lineNumber: 1, column: 1 },
      "package main",
      { language: "go" }
    );
    expect(goSuggestions.some((s) => s.label === "func")).toBe(true);

    const plainSuggestions = await engine.getSuggestions(
      "notes.txt",
      { lineNumber: 1, column: 1 },
      "plain text",
      { language: "plaintext" }
    );
    expect(plainSuggestions).toEqual([]);
  });

  it("exposes the context analyzer and prompt optimizer", () => {
    expect(engine.getContextAnalyzer()).toBeInstanceOf(ContextAnalyzer);
    expect(engine.getPromptOptimizer()).toBeInstanceOf(ChinesePromptOptimizer);
  });
});

describe("ChinesePromptOptimizer", () => {
  const optimizer = new ChinesePromptOptimizer();

  it("includes file, position, language, complexity, and variable context in the prompt", async () => {
    const analyzer = new ContextAnalyzer();
    const context = await analyzer.analyze({
      file: "src/app.ts",
      position: { lineNumber: 2, column: 1 },
      surroundingCode: ["const total = 42;", ""].join("\n"),
      language: "typescript",
    });

    const prompt = optimizer.optimizeCodeCompletion(context);

    expect(prompt).toContain("src/app.ts");
    expect(prompt).toContain("typescript");
    expect(prompt).toContain("total");
    expect(prompt).toContain("补全要求");
  });

  it("builds an error-analysis prompt with message and optional location", () => {
    const prompt = optimizer.optimizeErrorMessage({
      message: "TS2304: Cannot find name 'foo'",
      location: { lineNumber: 12, column: 5 },
      code: "TS2304",
    });

    expect(prompt).toContain("TS2304");
    expect(prompt).toContain("- 行: 12");
    expect(prompt).toContain("错误原因");
  });
});

describe("createMonacoCompletionProvider", () => {
  const fakeMonaco = {
    languages: {
      CompletionItemKind: {
        Function: 2,
        Variable: 6,
        Class: 5,
        Interface: 8,
        Keyword: 14,
        Snippet: 15,
        File: 17,
        Module: 9,
        Property: 10,
        Method: 1,
      },
    },
  } as unknown as typeof import("monaco-editor");

  function fakeModel(overrides: Partial<{
    language: string;
    word: { startColumn: number; endColumn: number };
  }> = {}) {
    return {
      uri: { fsPath: "D:/work/agent-ide/src/app.ts" },
      getLanguageId: () => overrides.language ?? "typescript",
      getValue: () => "const total = 42;\n",
      getWordUntilPosition: () => overrides.word ?? { startColumn: 1, endColumn: 1 },
    } as unknown as import("monaco-editor").editor.ITextModel;
  }

  it("maps engine suggestions to Monaco completion items", async () => {
    const engine = createIntelligentCompletionEngine();
    const provider = createMonacoCompletionProvider(engine, fakeMonaco);

    const suggestions =
      (
        await provider.provideCompletionItems(
          fakeModel(),
          { lineNumber: 2, column: 1 } as import("monaco-editor").Position,
          {} as import("monaco-editor").languages.CompletionContext,
          {} as import("monaco-editor").CancellationToken
        )
      )?.suggestions ?? [];

    expect(suggestions.length).toBeGreaterThan(0);
    for (const item of suggestions) {
      expect(item.range).toEqual({
        startLineNumber: 2,
        endLineNumber: 2,
        startColumn: 1,
        endColumn: 1,
      });
      expect(typeof item.insertText).toBe("string");
    }
  });

  it("returns no suggestions when the engine rejects", async () => {
    const failing = {
      getSuggestions: () => {
        throw new Error("engine down");
      },
    } as unknown as IntelligentCompletionEngine;
    const errors: unknown[] = [];
    const errorSpy = vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });

    const provider = createMonacoCompletionProvider(failing, fakeMonaco);
    const suggestions =
      (
        await provider.provideCompletionItems(
          fakeModel(),
          { lineNumber: 1, column: 1 } as import("monaco-editor").Position,
          {} as import("monaco-editor").languages.CompletionContext,
          {} as import("monaco-editor").CancellationToken
        )
      )?.suggestions ?? [];

    expect(suggestions).toEqual([]);
    expect(errors).toHaveLength(1);
    errorSpy.mockRestore();
  });
});
