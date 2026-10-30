import React from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence, MotionConfig } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  isWindows,
  isLinux,
  DRAG_REGION_ATTR,
  DRAG_REGION_STYLE,
} from "@/lib/platform";
import { isTextEditableTarget } from "@/utils/domUtils";
import { cn } from "@/lib/utils";
import { DialogLayerContext } from "@/components/ui/dialogLayer";

interface FullScreenPanelProps {
  isOpen: boolean;
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
  /**
   * 覆盖内容区滚动容器的内边距/间距类。默认 `px-6 py-6 space-y-6`。
   * 通过 `cn`(twMerge) 合并，传入如 `pt-3` 只覆盖顶部内边距，其余保持默认。
   */
  contentClassName?: string;
  /** Accessible name of the back button. */
  backLabel?: string;
}

// Open panels in open order. Only the top-most panel reacts to Escape, so a
// nested panel (speed test, common config, ...) does not also close its
// parent and discard the parent's unsaved edits (FE-PROV-41).
const openPanelStack: symbol[] = [];

const DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px - match App.tsx
const HEADER_HEIGHT = 64; // px - match App.tsx

/**
 * Reusable full-screen panel component
 * Handles portal rendering, header with back button, and footer
 * Uses solid theme colors without transparency
 */
export const FullScreenPanel: React.FC<FullScreenPanelProps> = ({
  isOpen,
  title,
  onClose,
  children,
  footer,
  contentClassName,
  backLabel,
}) => {
  const { t } = useTranslation();
  React.useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = "hidden";
    }
    return () => {
      document.body.style.overflow = "";
    };
  }, [isOpen]);

  const titleId = React.useId();
  const backButtonRef = React.useRef<HTMLButtonElement>(null);

  // Move focus into the panel on open and give it back on close, so keyboard
  // and screen-reader users are not left behind the overlay (FE-UI-12).
  React.useEffect(() => {
    if (!isOpen) return;
    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const frame = requestAnimationFrame(() => {
      const panel = backButtonRef.current?.closest('[role="dialog"]');
      if (panel && !panel.contains(document.activeElement)) {
        backButtonRef.current?.focus();
      }
    });
    return () => {
      cancelAnimationFrame(frame);
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, [isOpen]);

  // ESC 键关闭面板
  const onCloseRef = React.useRef(onClose);

  React.useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  React.useEffect(() => {
    if (!isOpen) return;

    const panelToken = Symbol("full-screen-panel");
    openPanelStack.push(panelToken);

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (openPanelStack[openPanelStack.length - 1] !== panelToken) {
          return;
        }
        // 子组件（例如 Radix 的 Select/Dialog/Dropdown）如果已经消费了 ESC，就不要再关闭整个面板
        if (event.defaultPrevented) {
          return;
        }

        if (isTextEditableTarget(event.target)) {
          return; // 让输入框自己处理 ESC（比如清空、失焦等）
        }

        event.stopPropagation(); // 阻止事件继续冒泡到 window，避免触发 App.tsx 的全局监听
        onCloseRef.current();
      }
    };

    // 使用冒泡阶段监听，让子组件（如 Radix UI）优先处理 ESC
    window.addEventListener("keydown", handleKeyDown, false);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, false);
      const index = openPanelStack.lastIndexOf(panelToken);
      if (index >= 0) openPanelStack.splice(index, 1);
    };
  }, [isOpen]);

  return createPortal(
    <MotionConfig reducedMotion="user">
      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="fixed inset-0 z-[60] flex flex-col"
            style={{ backgroundColor: "hsl(var(--background))" }}
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
          >
            <DialogLayerContext.Provider value="alert">
              {/* Drag region - match App.tsx. Linux 上 DRAG_BAR_HEIGHT=0，
              直接跳过整个元素；macOS 保留 28px 拖拽占位。 */}
              {DRAG_BAR_HEIGHT > 0 && (
                <div
                  data-tauri-drag-region
                  style={
                    {
                      WebkitAppRegion: "drag",
                      height: DRAG_BAR_HEIGHT,
                    } as React.CSSProperties
                  }
                />
              )}

              {/* Header - match App.tsx */}
              <div
                className="flex-shrink-0 flex items-center"
                {...DRAG_REGION_ATTR}
                style={
                  {
                    ...DRAG_REGION_STYLE,
                    backgroundColor: "hsl(var(--background))",
                    height: HEADER_HEIGHT,
                  } as React.CSSProperties
                }
              >
                <div
                  className="px-6 w-full flex items-center gap-4"
                  {...DRAG_REGION_ATTR}
                  style={{ ...DRAG_REGION_STYLE } as React.CSSProperties}
                >
                  <Button
                    ref={backButtonRef}
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={onClose}
                    aria-label={backLabel ?? t("common.back")}
                    className="rounded-lg select-none"
                    style={
                      { WebkitAppRegion: "no-drag" } as React.CSSProperties
                    }
                  >
                    <ArrowLeft className="h-4 w-4" />
                  </Button>
                  <h2
                    id={titleId}
                    className="text-lg font-semibold text-foreground select-none"
                  >
                    {title}
                  </h2>
                </div>
              </div>

              {/* Content */}
              <div className="flex-1 overflow-y-auto scroll-overlay">
                <div
                  className={cn("px-6 py-6 space-y-6 w-full", contentClassName)}
                >
                  {children}
                </div>
              </div>

              {/* Footer */}
              {footer && (
                <div
                  className="flex-shrink-0 py-4 border-t border-border-default"
                  style={{ backgroundColor: "hsl(var(--background))" }}
                >
                  <div className="px-6 flex items-center justify-end gap-3">
                    {footer}
                  </div>
                </div>
              )}
            </DialogLayerContext.Provider>
          </motion.div>
        )}
      </AnimatePresence>
    </MotionConfig>,
    document.body,
  );
};
