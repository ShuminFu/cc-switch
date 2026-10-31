import { useEffect, useState } from "react";
import type { Provider, Settings, VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { appCaps, useAppCaps } from "@/entities/app/registry";
import type { View } from "./views";
import { CommandPalette, useCommandPaletteShortcut } from "./CommandPalette";

interface AppCommandPaletteProps {
  activeApp: AppId;
  setActiveApp: (appId: AppId) => void;
  setCurrentView: (view: View) => void;
  /** 已绑定到当前 activeApp 的切换函数（来自 useProviderActions） */
  switchProvider: (provider: Provider) => Promise<unknown>;
  visibleApps: VisibleApps;
  settings: Settings | undefined;
  setTakeoverForApp: (args: {
    appType: string;
    enabled: boolean;
  }) => Promise<unknown>;
  isTakeoverActive: boolean;
  onOpenSettings: (tab: string) => void;
  onAddProvider: () => void;
}

/** 应用外壳中的命令面板：持有打开状态、Cmd/Ctrl-K 快捷键与跨应用切换逻辑 */
export function AppCommandPalette({
  activeApp,
  setActiveApp,
  setCurrentView,
  switchProvider,
  visibleApps,
  settings,
  setTakeoverForApp,
  isTakeoverActive,
  onOpenSettings,
  onAddProvider,
}: AppCommandPaletteProps) {
  const activeAppCaps = useAppCaps(activeApp);
  const [isOpen, setIsOpen] = useState(false);
  // 命令面板跨应用切换：先切换 activeApp，待 useProviderActions 绑定到目标应用后再执行
  const [pendingSwitch, setPendingSwitch] = useState<{
    appId: AppId;
    provider: Provider;
  } | null>(null);

  useCommandPaletteShortcut(isOpen, setIsOpen);

  useEffect(() => {
    if (!pendingSwitch) return;
    setPendingSwitch(null);
    // 与 setActiveApp 同批提交，此时 activeApp 应已是目标应用；
    // 若被可见性回退等逻辑改写则放弃，避免切换到错误应用
    if (pendingSwitch.appId === activeApp) {
      void switchProvider(pendingSwitch.provider);
    }
  }, [pendingSwitch, activeApp, switchProvider]);

  const selectApp = (appId: AppId) => {
    setActiveApp(appId);
    setCurrentView("providers");
  };

  const switchProviderInApp = (appId: AppId, provider: Provider) => {
    selectApp(appId);
    // 累加模式应用没有“当前供应商”，只跳转到列表
    if (!appCaps(appId).additive) {
      setPendingSwitch({ appId, provider });
    }
  };

  const toggleTakeover = () =>
    void setTakeoverForApp({
      appType: activeApp,
      enabled: !isTakeoverActive,
    }).catch((error) => {
      console.error("[CommandPalette] Toggle takeover failed:", error);
    });

  return (
    <CommandPalette
      open={isOpen}
      onOpenChange={setIsOpen}
      visibleApps={visibleApps}
      activeApp={activeApp}
      onSwitchProvider={switchProviderInApp}
      onSelectApp={selectApp}
      onOpenSettings={() => onOpenSettings("general")}
      onAddProvider={onAddProvider}
      onToggleTakeover={
        settings?.enableLocalProxy && activeAppCaps.proxyTakeover
          ? toggleTakeover
          : undefined
      }
      isTakeoverActive={isTakeoverActive}
    />
  );
}
