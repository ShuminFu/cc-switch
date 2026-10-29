import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "@/types";
import { isLinux, isWindows } from "@/lib/platform";
import { extractErrorMessage } from "@/utils/errorUtils";
import { useWindowMaximizedSync } from "@/hooks/useWindowMaximizedSync";

const DEFAULT_DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px
const APP_CONTROLS_DRAG_BAR_HEIGHT = 32; // px
export const HEADER_HEIGHT = 64; // px

export interface WindowChrome {
  /** Linux 下使用应用内窗口控制按钮（关闭系统装饰） */
  useAppWindowControls: boolean;
  dragBarHeight: number;
  /** 内容区顶部偏移 = 拖拽条 + 顶栏 */
  contentTopOffset: number;
  isWindowMaximized: boolean;
  minimize: () => Promise<void>;
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
}

/**
 * 主窗口外框状态：拖拽条高度、窗口装饰同步、最大化状态与窗口控制操作。
 */
export function useWindowChrome(settings: Settings | undefined): WindowChrome {
  const { t } = useTranslation();
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  const useAppWindowControls =
    isLinux() && (settings?.useAppWindowControls ?? false);
  const dragBarHeight = useAppWindowControls
    ? APP_CONTROLS_DRAG_BAR_HEIGHT
    : DEFAULT_DRAG_BAR_HEIGHT;

  useWindowMaximizedSync(setIsWindowMaximized);

  const settingsLoaded = Boolean(settings);
  useEffect(() => {
    // settings 未加载时跳过，避免用 fallback false 覆盖 Rust 侧已设好的装饰状态
    if (!settingsLoaded) return;

    const syncWindowDecorations = async () => {
      try {
        await getCurrentWindow().setDecorations(!useAppWindowControls);
      } catch (error) {
        console.error("[App] Failed to update window decorations", error);
      }
    };

    void syncWindowDecorations();
  }, [useAppWindowControls, settingsLoaded]);

  const notifyWindowControlError = (error: unknown) => {
    toast.error(
      t("notifications.windowControlFailed", {
        defaultValue: "窗口控制失败：{{error}}",
        error: extractErrorMessage(error),
      }),
    );
  };

  const minimize = async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      console.error("[App] Failed to minimize window", error);
      notifyWindowControlError(error);
    }
  };

  const toggleMaximize = async () => {
    try {
      const currentWindow = getCurrentWindow();
      await currentWindow.toggleMaximize();
      setIsWindowMaximized(await currentWindow.isMaximized());
    } catch (error) {
      console.error("[App] Failed to toggle maximize", error);
      notifyWindowControlError(error);
    }
  };

  const close = async () => {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      console.error("[App] Failed to close window", error);
      notifyWindowControlError(error);
    }
  };

  return {
    useAppWindowControls,
    dragBarHeight,
    contentTopOffset: dragBarHeight + HEADER_HEIGHT,
    isWindowMaximized,
    minimize,
    toggleMaximize,
    close,
  };
}
