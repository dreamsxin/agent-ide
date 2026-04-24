import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * 监听 Tauri 后端事件
 * 自动在组件卸载时取消监听
 */
export function useTauriEvent<T>(
  event: string,
  handler: (payload: T) => void
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const setup = async () => {
      try {
        unlisten = await listen<T>(event, (e) => {
          handlerRef.current(e.payload);
        });
      } catch (err) {
        console.warn(`[useTauriEvent] Failed to listen to "${event}":`, err);
      }
    };

    setup();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [event]);
}

/**
 * 组合监听多个 Tauri 事件
 */
export function useTauriEvents<T extends Record<string, unknown>>(
  handlers: {
    [K in keyof T]: (payload: T[K]) => void;
  }
) {
  const keys = Object.keys(handlers) as (keyof T)[];

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    keys.forEach((event) => {
      const eventName = event as string;
      listen(eventName, (e: { payload: unknown }) => {
        handlers[event](e.payload as T[typeof event]);
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((err) =>
          console.warn(`[useTauriEvents] Failed to listen to "${eventName}":`, err)
        );
    });

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);
}
