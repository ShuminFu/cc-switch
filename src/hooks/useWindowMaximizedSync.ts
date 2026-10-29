import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * 同步当前窗口的最大化状态：挂载时读取一次，并在窗口尺寸变化时重新读取。
 * onResized 的注册是异步的；若组件在注册完成前卸载（StrictMode 下必然发生），
 * 注册完成后立即注销，避免监听器泄漏。
 */
export function useWindowMaximizedSync(
  onChange: (maximized: boolean) => void,
): void {
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    let active = true;
    let unlistenResize: (() => void) | undefined;

    const setupWindowStateSync = async () => {
      try {
        const currentWindow = getCurrentWindow();
        const syncWindowMaximizedState = async () => {
          const maximized = await currentWindow.isMaximized();
          if (active) {
            onChangeRef.current(maximized);
          }
        };

        await syncWindowMaximizedState();
        if (!active) return;
        const off = await currentWindow.onResized(() => {
          void syncWindowMaximizedState();
        });
        if (!active) {
          off();
          return;
        }
        unlistenResize = off;
      } catch (error) {
        console.error("[App] Failed to sync window maximized state", error);
      }
    };

    void setupWindowStateSync();
    return () => {
      active = false;
      unlistenResize?.();
    };
  }, []);
}
