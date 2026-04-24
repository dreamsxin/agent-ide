import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { GitStatus } from "../types/project";

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
      const status = await invoke<GitStatus>("git_status", { path });
      set({ status, loading: false });
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
    }
  },

  fetchDiff: async (path: string, file?: string) => {
    set({ loading: true, error: null });
    try {
      const diff = await invoke<string>("git_diff", { path, file: file ?? null });
      set({ diff, loading: false });
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
    }
  },

  commit: async (path: string, message: string, files?: string[]) => {
    set({ loading: true, error: null });
    try {
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
