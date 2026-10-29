import { useEffect, useRef } from "react";
import { isTextEditableTarget } from "@/utils/domUtils";
import { getBackView, type View } from "./views";

/**
 * 全局视图快捷键：Cmd/Ctrl+, 打开设置；Esc 返回上一级
 * （弹窗打开、输入框聚焦或事件已被处理时不响应 Esc）。
 */
export function useViewShortcuts(
  currentView: View,
  setCurrentView: (view: View) => void,
): void {
  const currentViewRef = useRef(currentView);
  const setViewRef = useRef(setCurrentView);

  useEffect(() => {
    currentViewRef.current = currentView;
    setViewRef.current = setCurrentView;
  }, [currentView, setCurrentView]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setViewRef.current("settings");
        return;
      }

      if (event.key !== "Escape" || event.defaultPrevented) return;

      if (document.body.style.overflow === "hidden") return;

      const view = currentViewRef.current;
      if (view === "providers") return;

      if (isTextEditableTarget(event.target)) return;

      event.preventDefault();
      setViewRef.current(getBackView(view));
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);
}
