import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffKind, GitStatus } from "../types/project";
import { isTauriRuntime } from "../utils/tauri";

interface GitStore {
  status: GitStatus | null;
  diff: string | null;
  loading: boolean;
  error: string | null;

  fetchStatus: (path: string) => Promise<void>;
  fetchDiff: (path: string, file?: string, kind?: GitDiffKind) => Promise<void>;
  commit: (path: string, message: string, files?: string[]) => Promise<string | null>;
  stageFiles: (path: string, files: string[]) => Promise<boolean>;
  unstageFiles: (path: string, files: string[]) => Promise<boolean>;
  discardFiles: (path: string, files: string[]) => Promise<boolean>;
  checkoutBranch: (path: string, branch: string, create?: boolean) => Promise<boolean>;
  fetch: (path: string, remote?: string) => Promise<boolean>;
  pull: (path: string, remote?: string) => Promise<boolean>;
  push: (path: string, remote?: string) => Promise<boolean>;
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

  fetchDiff: async (path: string, file?: string, kind: GitDiffKind = "all") => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ diff: null, loading: false, error: "Git diff is available in the Tauri app runtime." });
        return;
      }
      const diff = await invoke<string>("git_diff", { path, file: file ?? null, kind });
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

  stageFiles: async (path, files) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git stage is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_stage_files", { path, files });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  unstageFiles: async (path, files) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git unstage is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_unstage_files", { path, files });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  discardFiles: async (path, files) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git discard is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_discard_files", { path, files });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  checkoutBranch: async (path, branch, create = false) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git branch operations are available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_checkout_branch", { path, branch, create });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  fetch: async (path, remote) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git fetch is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_fetch", { path, remote: remote ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  pull: async (path, remote) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git pull is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_pull", { path, remote: remote ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  push: async (path, remote) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: "Git push is available in the Tauri app runtime." });
        return false;
      }
      await invoke("git_push", { path, remote: remote ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  clearDiff: () => set({ diff: null }),
}));
