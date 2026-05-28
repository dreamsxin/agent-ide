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
  message: string;
  diagnosticSummaries: LspDiagnosticSummary[];
  setStatus: (status: LspStatus, message?: string) => void;
  setDiagnosticSummary: (summary: LspDiagnosticSummary) => void;
}

export const useLspStore = create<LspStore>((set) => ({
  status: "idle",
  message: "Language server is not initialized.",
  diagnosticSummaries: [],
  setStatus: (status, message) =>
    set({
      status,
      message: message ?? defaultMessage(status),
    }),
  setDiagnosticSummary: (summary) =>
    set((state) => {
      const next = state.diagnosticSummaries.filter((item) => item.file !== summary.file);
      return { diagnosticSummaries: [summary, ...next].slice(0, 8) };
    }),
}));

function defaultMessage(status: LspStatus) {
  switch (status) {
    case "checking":
      return "Checking language server...";
    case "ready":
      return "Language server ready.";
    case "unavailable":
      return "Language server unavailable. Open an LSP-supported file for install guidance.";
    case "error":
      return "Language server failed.";
    case "idle":
    default:
      return "Language server is not initialized.";
  }
}
