import { useEffect, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Maximize2, Minimize2, Minus, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/button";
import { extractErrorMessage } from "@/utils/errorUtils";
import { NO_DRAG_STYLE } from "./PageHeader";

interface TitleBarProps {
  /** Height of the drag bar in px (0 hides it unless controls are shown). */
  height: number;
  /** Render in-app minimize/maximize/close buttons (Linux option). */
  showWindowControls: boolean;
  /**
   * Whether settings have loaded. Window decorations are only synced after
   * that, so the fallback `false` does not override what Rust already set.
   */
  settingsLoaded: boolean;
}

const DRAG_STYLE = { WebkitAppRegion: "drag" } as CSSProperties;

/** Window drag bar and optional in-app window controls. */
export function TitleBar({
  height,
  showWindowControls,
  settingsLoaded,
}: TitleBarProps) {
  const { t } = useTranslation();
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  useEffect(() => {
    let active = true;
    let unlistenResize: (() => void) | undefined;

    const setupWindowStateSync = async () => {
      try {
        const currentWindow = getCurrentWindow();
        const syncWindowMaximizedState = async () => {
          const maximized = await currentWindow.isMaximized();
          if (active) {
            setIsWindowMaximized(maximized);
          }
        };

        await syncWindowMaximizedState();
        unlistenResize = await currentWindow.onResized(() => {
          void syncWindowMaximizedState();
        });
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

  useEffect(() => {
    if (!settingsLoaded) return;

    const syncWindowDecorations = async () => {
      try {
        await getCurrentWindow().setDecorations(!showWindowControls);
      } catch (error) {
        console.error("[App] Failed to update window decorations", error);
      }
    };

    void syncWindowDecorations();
  }, [showWindowControls, settingsLoaded]);

  const notifyWindowControlError = (error: unknown) => {
    toast.error(
      t("notifications.windowControlFailed", {
        defaultValue: "窗口控制失败：{{error}}",
        error: extractErrorMessage(error),
      }),
    );
  };

  const handleWindowMinimize = async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      console.error("[App] Failed to minimize window", error);
      notifyWindowControlError(error);
    }
  };

  const handleWindowToggleMaximize = async () => {
    try {
      const currentWindow = getCurrentWindow();
      await currentWindow.toggleMaximize();
      setIsWindowMaximized(await currentWindow.isMaximized());
    } catch (error) {
      console.error("[App] Failed to toggle maximize", error);
      notifyWindowControlError(error);
    }
  };

  const handleWindowClose = async () => {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      console.error("[App] Failed to close window", error);
      notifyWindowControlError(error);
    }
  };

  if (height <= 0 && !showWindowControls) {
    return null;
  }

  return (
    <div
      className="fixed top-0 left-0 right-0 z-[70] flex items-center justify-end px-2"
      data-tauri-drag-region
      style={{ ...DRAG_STYLE, height }}
    >
      {showWindowControls && (
        <div className="flex items-center gap-1" style={NO_DRAG_STYLE}>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void handleWindowMinimize()}
            title={t("header.windowMinimize")}
            aria-label={t("header.windowMinimize")}
            className="h-7 w-7"
          >
            <Minus className="w-4 h-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void handleWindowToggleMaximize()}
            title={
              isWindowMaximized
                ? t("header.windowRestore")
                : t("header.windowMaximize")
            }
            aria-label={
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
            onClick={() => void handleWindowClose()}
            title={t("header.windowClose")}
            aria-label={t("header.windowClose")}
            className="h-7 w-7 hover:bg-red-500/15 hover:text-red-500"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>
      )}
    </div>
  );
}
