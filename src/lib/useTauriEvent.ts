import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppEventName, AppEvents } from "@/lib/events";

/**
 * Subscribe to a backend event for the lifetime of the component. The event
 * name must be a key of the `AppEvents` contract (FE-ARCH-19), and the
 * payload is typed from it. Handles the async `listen` registration so a
 * StrictMode double mount or an unmount before registration finishes never
 * leaks a listener. The latest `handler` is always called.
 */
export function useTauriEvent<K extends AppEventName>(
  eventName: K,
  handler: (payload: AppEvents[K]) => void | Promise<void>,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    void (async () => {
      try {
        const off = await listen<AppEvents[K]>(eventName, (event) => {
          void handlerRef.current(event.payload);
        });
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      } catch (error) {
        console.error(`Failed to subscribe ${eventName} event`, error);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [eventName]);
}
