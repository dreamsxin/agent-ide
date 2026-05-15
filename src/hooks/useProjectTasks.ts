import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PROJECT_TASKS, useTaskStore, type ProjectTaskDefinition } from "../stores/useTaskStore";
import { isTauriRuntime } from "../utils/tauri";

export function useProjectTasks() {
  const discoveredTasks = useTaskStore((s) => s.discoveredTasks);
  const loading = useTaskStore((s) => s.taskDiscoveryLoading);
  const loaded = useTaskStore((s) => s.taskDiscoveryLoaded);
  const error = useTaskStore((s) => s.taskDiscoveryError);
  const setDiscoveredTasks = useTaskStore((s) => s.setDiscoveredTasks);
  const setTaskDiscoveryState = useTaskStore((s) => s.setTaskDiscoveryState);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    if (loading || loaded) return;

    let cancelled = false;
    setTaskDiscoveryState(true);
    invoke<ProjectTaskDefinition[]>("discover_project_tasks")
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
  }, [loaded, loading, setDiscoveredTasks, setTaskDiscoveryState]);

  return {
    tasks: discoveredTasks.length > 0 ? discoveredTasks : PROJECT_TASKS,
    discoveredTasks,
    loading,
    error,
    usingFallback: discoveredTasks.length === 0,
  };
}
