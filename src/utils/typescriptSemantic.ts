import type * as Monaco from "monaco-editor";
import type { FileTab } from "../types/editor";

const TS_LANGUAGES = new Set(["typescript", "javascript"]);
let configured = false;

export function configureTypeScriptSemantic(monaco: typeof Monaco) {
  if (configured) return;
  configured = true;

  const compilerOptions: Monaco.languages.typescript.CompilerOptions = {
    allowJs: true,
    checkJs: false,
    jsx: monaco.languages.typescript.JsxEmit.ReactJSX,
    moduleResolution: monaco.languages.typescript.ModuleResolutionKind.NodeJs,
    target: monaco.languages.typescript.ScriptTarget.ES2020,
    module: monaco.languages.typescript.ModuleKind.ESNext,
    allowNonTsExtensions: true,
    esModuleInterop: true,
    resolveJsonModule: true,
    strict: true,
    noEmit: true,
  };

  const diagnosticsOptions: Monaco.languages.typescript.DiagnosticsOptions = {
    noSyntaxValidation: false,
    noSemanticValidation: false,
    noSuggestionDiagnostics: false,
  };

  monaco.languages.typescript.typescriptDefaults.setCompilerOptions(compilerOptions);
  monaco.languages.typescript.javascriptDefaults.setCompilerOptions(compilerOptions);
  monaco.languages.typescript.typescriptDefaults.setDiagnosticsOptions(diagnosticsOptions);
  monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
    ...diagnosticsOptions,
    noSemanticValidation: true,
  });
  monaco.languages.typescript.typescriptDefaults.setEagerModelSync(true);
  monaco.languages.typescript.javascriptDefaults.setEagerModelSync(true);
}

export function isTypeScriptSemanticLanguage(language: string) {
  return TS_LANGUAGES.has(language);
}

export function filePathToMonacoUri(monaco: typeof Monaco, path: string) {
  const normalized = path.replace(/\\/g, "/");
  const withLeadingSlash = /^[A-Za-z]:\//.test(normalized) ? `/${normalized}` : normalized;
  return monaco.Uri.parse(`file://${encodeURI(withLeadingSlash)}`);
}

export function ensureOpenFileModels(
  monaco: typeof Monaco,
  openFiles: FileTab[],
  fileContents: Record<string, string>
) {
  for (const file of openFiles) {
    const uri = filePathToMonacoUri(monaco, file.path);
    const content = fileContents[file.path];
    if (content === undefined) continue;
    const existing = monaco.editor.getModel(uri);
    if (existing) {
      if (existing.getValue() !== content) {
        existing.setValue(content);
      }
      continue;
    }
    monaco.editor.createModel(content, file.language, uri);
  }
}
