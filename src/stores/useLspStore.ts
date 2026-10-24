import { create } from "zustand";

export type LspStatus = "idle" | "checking" | "ready" | "unavailable" | "error";

export interface LspDiagnosticSummary {
  file: string;
  error: number;
  warning: number;
  info: number;
}

interface LspStore {
  status: LspStatus;
  /**
   * 具体那一句说明；`null` = 没有额外的话，界面按 `status` 显示默认说明。
   *
   * 以前这里存的是一句英文，而且同一句"还没启动"在这个文件里写了两遍（初始值和
   * `defaultMessage` 的 default 分支）。按状态给默认说明是**显示**的事，不是 store 的事：
   * 放在这里就得在 store 里认识界面语言，而 store 是被事件和命令写的，不是被界面写的。
   */
  message: string | null;
  diagnosticSummaries: LspDiagnosticSummary[];
  setStatus: (status: LspStatus, message?: string) => void;
  setDiagnosticSummary: (summary: LspDiagnosticSummary) => void;
}

export const useLspStore = create<LspStore>((set) => ({
  status: "idle",
  message: null,
  diagnosticSummaries: [],
  setStatus: (status, message) => set({ status, message: message ?? null }),
  setDiagnosticSummary: (summary) =>
    set((state) => {
      const next = state.diagnosticSummaries.filter((item) => item.file !== summary.file);
      return { diagnosticSummaries: [summary, ...next].slice(0, 8) };
    }),
}));
