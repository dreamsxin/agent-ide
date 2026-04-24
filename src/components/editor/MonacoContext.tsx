import { createContext, useContext } from "react";
import type { editor } from "monaco-editor";

/** Monaco editor instance + namespace shared across AI-layer components */
export interface MonacoContextValue {
  editor: editor.IStandaloneCodeEditor | null;
  monaco: typeof import("monaco-editor") | null;
}

export const MonacoContext = createContext<MonacoContextValue>({
  editor: null,
  monaco: null,
});

export function useMonacoContext() {
  return useContext(MonacoContext);
}
