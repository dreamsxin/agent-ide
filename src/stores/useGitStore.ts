import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { GitStatus } from "../types/project";
import { isTauriRuntime } from "../utils/tauri";

interface GitStore {
  status: GitStatus | null;
  diff: string | null;
  loading: boolean;
  error: string | null;

  fetchStatus: (path: string) => Promise<void>;
  fetchDiff: (path: string, file?: string) => Promise<void>;
  commit: (path: string, message: string, files?: string[]) => Promise<string | null>;
  clearDiff: () => void;
}

export const useGitStore = create<GitStore>((set) => ({
  status: null,
  diff: null,
  loading: false,
  error: null,

  fetchStatus: async (path: string) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ status: null, loading: false, error: "Git is available in the Tauri app runtime." });
        return;
      }
      const status = await invoke<GitStatus>("git_status", { path });
      set({ status, loading: false });
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
    }
  },

  fetchDiff: async (path: string, file?: string) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ diff: null, loading: false, error: "Git diff is available in the Tauri app runtime." });
        return;
      }
      const diff = await invoke<string>("git_diff", { path, file: file ?? null });
      set({ diff, loading: false });
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
    }
  },

  commit: async (path: string, message: string, files?: string[]) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git commit is available in the Tauri app runtime." });
        return null;
      }
      const oid = await invoke<string>("git_commit", {
        path,
        message,
        files: files ?? null,
      });
      set({ loading: false });
      return oid;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return null;
    }
  },

  clearDiff: () => set({ diff: null }),
}));
