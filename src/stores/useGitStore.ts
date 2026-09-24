import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { GitCredentials, GitDiffKind, GitStatus } from "../types/project";
import { isTauriRuntime } from "../utils/tauri";
import { translate, useLocaleStore } from "../i18n";

/**
 * 浏览器里没有 git —— 十一个动作共用这一句。
 *
 * 之前每个动作各写一句："Git is available in…"、"Git commit is available in…"、
 * "Git stage is available in…"，十一种拼法说的是同一件事。区分哪个动作对用户没有用
 * （他知道自己点了什么），但十一份拷贝意味着翻译时能漏掉十份。
 */
function needsTauriError(): string {
  return translate(useLocaleStore.getState().locale, "git.needsTauri");
}

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
  checkoutRemoteBranch: (path: string, remoteBranch: string, localBranch?: string) => Promise<boolean>;
  fetch: (path: string, remote?: string, credentials?: GitCredentials | null) => Promise<boolean>;
  pull: (path: string, remote?: string, credentials?: GitCredentials | null) => Promise<boolean>;
  push: (path: string, remote?: string, credentials?: GitCredentials | null) => Promise<boolean>;
  resolveConflict: (path: string, file: string, resolution: "current" | "incoming" | "both") => Promise<boolean>;
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
        set({ status: null, loading: false, error: needsTauriError() });
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
        set({ diff: null, loading: false, error: needsTauriError() });
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
        set({ loading: false, error: needsTauriError() });
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
        set({ loading: false, error: needsTauriError() });
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
        set({ loading: false, error: needsTauriError() });
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
        set({ loading: false, error: needsTauriError() });
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
        set({ loading: false, error: needsTauriError() });
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

  checkoutRemoteBranch: async (path, remoteBranch, localBranch) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: needsTauriError() });
        return false;
      }
      await invoke("git_checkout_remote_branch", {
        path,
        remoteBranch,
        localBranch: localBranch?.trim() || null,
      });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  fetch: async (path, remote, credentials) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: needsTauriError() });
        return false;
      }
      await invoke("git_fetch", { path, remote: remote ?? null, credentials: credentials ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  pull: async (path, remote, credentials) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: needsTauriError() });
        return false;
      }
      await invoke("git_pull", { path, remote: remote ?? null, credentials: credentials ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  push: async (path, remote, credentials) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: needsTauriError() });
        return false;
      }
      await invoke("git_push", { path, remote: remote ?? null, credentials: credentials ?? null });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  resolveConflict: async (path, file, resolution) => {
    set({ loading: true, error: null });
    try {
      if (!isTauriRuntime()) {
        set({ loading: false, error: needsTauriError() });
        return false;
      }
      await invoke("git_resolve_conflict", { path, file, resolution });
      set({ loading: false });
      return true;
    } catch (err: unknown) {
      set({ error: String(err), loading: false });
      return false;
    }
  },

  clearDiff: () => set({ diff: null }),
}));
