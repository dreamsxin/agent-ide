import { create } from "zustand";

export type LspStatus = "idle" | "checking" | "ready" | "unavailable" | "error";

interface LspStore {
  status: LspStatus;
  message: string;
  setStatus: (status: LspStatus, message?: string) => void;
}

export const useLspStore = create<LspStore>((set) => ({
  status: "idle",
  message: "TypeScript LSP is not initialized.",
  setStatus: (status, message) =>
    set({
      status,
      message: message ?? defaultMessage(status),
    }),
}));

function defaultMessage(status: LspStatus) {
  switch (status) {
    case "checking":
      return "Checking TypeScript language server...";
    case "ready":
      return "TypeScript LSP ready.";
    case "unavailable":
      return "TypeScript LSP unavailable. Install: npm install -g typescript typescript-language-server";
    case "error":
      return "TypeScript LSP failed.";
    case "idle":
    default:
      return "TypeScript LSP is not initialized.";
  }
}
