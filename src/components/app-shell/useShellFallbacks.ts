import { useEffect } from "react";
import type { Settings, VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { ALL_APP_IDS } from "@/entities/app/registry";
import type { View } from "./views";

const DEFAULT_VISIBLE_APPS: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  grokbuild: true,
  opencode: true,
  openclaw: true,
  hermes: true,
};

interface ShellFallbackOptions {
  settings: Settings | undefined;
  activeApp: AppId;
  setActiveApp: (appId: AppId) => void;
  currentView: View;
  setCurrentView: (view: View) => void;
  /** 共享功能应用是否支持会话视图 */
  sessionsSupported: boolean;
}

/**
 * 应用外壳回退：当前应用被隐藏时切到第一个可见应用；
 * 切到不支持会话的应用时从会话视图回到供应商列表。返回生效的可见应用表。
 */
export function useShellFallbacks({
  settings,
  activeApp,
  setActiveApp,
  currentView,
  setCurrentView,
  sessionsSupported,
}: ShellFallbackOptions): VisibleApps {
  const visibleApps = settings?.visibleApps ?? DEFAULT_VISIBLE_APPS;

  useEffect(() => {
    if (!visibleApps[activeApp]) {
      setActiveApp(ALL_APP_IDS.find((id) => visibleApps[id]) ?? "claude");
    }
  }, [visibleApps, activeApp, setActiveApp]);

  // Fallback from sessions view when switching to an app without session support
  useEffect(() => {
    if (currentView === "sessions" && !sessionsSupported) {
      setCurrentView("providers");
    }
  }, [sessionsSupported, currentView, setCurrentView]);

  return visibleApps;
}
