import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLayoutStore } from "../stores/useLayoutStore";
import { PROJECT_TASKS, useTaskStore, type ProjectTaskDefinition } from "../stores/useTaskStore";
import { isTauriRuntime } from "../utils/tauri";

export function useProjectTasks() {
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const discoveredTasks = useTaskStore((s) => s.discoveredTasks);
  const loading = useTaskStore((s) => s.taskDiscoveryLoading);
  const error = useTaskStore((s) => s.taskDiscoveryError);
  const setDiscoveredTasks = useTaskStore((s) => s.setDiscoveredTasks);
  const setTaskDiscoveryState = useTaskStore((s) => s.setTaskDiscoveryState);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    if (!workspacePath) {
      setDiscoveredTasks([]);
      setTaskDiscoveryState(false, null, false);
      return;
    }

    let cancelled = false;
    setDiscoveredTasks([]);
    setTaskDiscoveryState(true);
    invoke<ProjectTaskDefinition[]>("discover_project_tasks", { path: workspacePath })
      .then((tasks) => {
        if (!cancelled) {
          setDiscoveredTasks(tasks);
          setTaskDiscoveryState(false, null, true);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setDiscoveredTasks([]);
          setTaskDiscoveryState(false, String(err), true);
        }
      })
      .finally(() => {
        if (!cancelled) setTaskDiscoveryState(false);
      });

    return () => {
      cancelled = true;
    };
  }, [workspacePath, setDiscoveredTasks, setTaskDiscoveryState]);

  return {
    tasks: discoveredTasks.length > 0 ? discoveredTasks : PROJECT_TASKS,
    discoveredTasks,
    loading,
    error,
    usingFallback: discoveredTasks.length === 0,
  };
}
