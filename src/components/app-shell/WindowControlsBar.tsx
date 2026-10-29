import { useTranslation } from "react-i18next";
import { Maximize2, Minimize2, Minus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { WindowChrome } from "@/hooks/useWindowChrome";
import { NO_DRAG_STYLE, type AppRegionStyle } from "./dragRegion";

/**
 * 顶部拖拽条；Linux 启用应用内窗口控制时在右侧显示最小化 / 最大化 / 关闭按钮。
 */
export function WindowControlsBar({ chrome }: { chrome: WindowChrome }) {
  const { t } = useTranslation();
  const { dragBarHeight, useAppWindowControls, isWindowMaximized } = chrome;

  if (dragBarHeight <= 0 && !useAppWindowControls) return null;

  const barStyle: AppRegionStyle = {
    WebkitAppRegion: "drag",
    height: dragBarHeight,
  };

  return (
    <div
      className="fixed top-0 left-0 right-0 z-[70] flex items-center justify-end px-2"
      data-tauri-drag-region
      style={barStyle}
    >
      {useAppWindowControls && (
        <div className="flex items-center gap-1" style={NO_DRAG_STYLE}>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void chrome.minimize()}
            title={t("header.windowMinimize")}
            className="h-7 w-7"
          >
            <Minus className="w-4 h-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void chrome.toggleMaximize()}
            title={
              isWindowMaximized
                ? t("header.windowRestore")
                : t("header.windowMaximize")
            }
            className="h-7 w-7"
          >
            {isWindowMaximized ? (
              <Minimize2 className="w-4 h-4" />
            ) : (
              <Maximize2 className="w-4 h-4" />
            )}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void chrome.close()}
            title={t("header.windowClose")}
            className="h-7 w-7 hover:bg-red-500/15 hover:text-red-500"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>
      )}
    </div>
  );
}
