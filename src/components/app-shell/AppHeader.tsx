import { useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  BarChart2,
  Plus,
  Settings as SettingsIcon,
} from "lucide-react";
import type { Settings, VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import type { AppCaps } from "@/entities/app/registry";
import type { SkillsPageSource } from "@/components/skills/SkillsPage";
import { useAutoCompact } from "@/hooks/useAutoCompact";
import { HEADER_HEIGHT } from "@/hooks/useWindowChrome";
import { cn } from "@/lib/utils";
import { DRAG_REGION_ATTR } from "@/lib/platform";
import { Button } from "@/components/ui/button";
import { AppSwitcher } from "@/components/AppSwitcher";
import { UpdateBadge } from "@/components/UpdateBadge";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import { AppFeatureNav, type FeatureNavTarget } from "./AppFeatureNav";
import { HeaderViewActions } from "./HeaderViewActions";
import { NO_DRAG_STYLE, PLATFORM_DRAG_STYLE } from "./dragRegion";
import type { ShellPanelRefs } from "./panelRefs";
import { VIEW_TITLES, getBackView, type View } from "./views";

const ADD_ACTION_BUTTON_CLASS =
  "bg-orange-500 hover:bg-orange-600 dark:bg-orange-500 dark:hover:bg-orange-600 text-white shadow-lg shadow-orange-500/30 dark:shadow-orange-500/40 rounded-full w-8 h-8";

export interface AppHeaderProps {
  currentView: View;
  onNavigate: (view: View) => void;
  /** 打开设置页的指定标签页 */
  onOpenSettings: (tab: string) => void;
  activeApp: AppId;
  onSwitchApp: (app: AppId) => void;
  visibleApps: VisibleApps;
  activeAppCaps: AppCaps;
  sharedFeatureApp: AppId;
  sharedFeatureCaps: AppCaps;
  settings: Settings | undefined;
  isProxyRunning: boolean;
  isTakeoverActive: boolean;
  dragBarHeight: number;
  panelRefs: ShellPanelRefs;
  skillsDiscoverySource: SkillsPageSource;
  onOpenSkillsDiscovery: () => void;
  onOpenHermesWebUI: () => void;
  onAddProvider: () => void;
}

/** 子视图：返回按钮 + 视图标题 */
function ViewTitleBar({
  view,
  sharedFeatureApp,
  onNavigate,
}: {
  view: Exclude<View, "providers">;
  sharedFeatureApp: AppId;
  onNavigate: (view: View) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2">
      <Button
        aria-label={t("common.back")}
        variant="outline"
        size="icon"
        onClick={() => onNavigate(getBackView(view))}
        className="mr-2 rounded-lg"
      >
        <ArrowLeft className="w-4 h-4" />
      </Button>
      <h1 className="text-lg font-semibold">
        {VIEW_TITLES[view](t, { sharedFeatureApp })}
      </h1>
    </div>
  );
}

/** 首页：品牌链接 + 设置 / 更新 / 使用统计入口 */
function BrandBar({
  isTakeoverActive,
  isProxyRunning,
  onOpenSettings,
}: Pick<
  AppHeaderProps,
  "isTakeoverActive" | "isProxyRunning" | "onOpenSettings"
>) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2">
      <div className="relative inline-flex items-center">
        <a
          href="https://ccswitch.io"
          target="_blank"
          rel="noreferrer"
          className={cn(
            "text-xl font-semibold transition-colors",
            isProxyRunning && isTakeoverActive
              ? "text-emerald-500 hover:text-emerald-600 dark:text-emerald-400 dark:hover:text-emerald-300"
              : "text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300",
          )}
        >
          CC Switch
        </a>
      </div>
      <Button
        variant="ghost"
        size="icon"
        onClick={() => onOpenSettings("general")}
        title={t("common.settings")}
        className="hover:bg-black/5 dark:hover:bg-white/5"
      >
        <SettingsIcon className="w-4 h-4" />
      </Button>
      <UpdateBadge onClick={() => onOpenSettings("about")} />
      {isTakeoverActive && (
        <Button
          variant="ghost"
          size="icon"
          onClick={() => onOpenSettings("usage")}
          title={t("usage.title", {
            defaultValue: "使用统计",
          })}
          className="hover:bg-black/5 dark:hover:bg-white/5"
        >
          <BarChart2 className="w-4 h-4" />
        </Button>
      )}
    </div>
  );
}

/** 首页的代理 / 故障转移开关（累加模式应用不显示） */
function ProxyControls({
  activeApp,
  settings,
}: Pick<AppHeaderProps, "activeApp" | "settings">) {
  const isClaudeDesktop = activeApp === "claude-desktop";
  return (
    <div className="flex shrink-0 items-center gap-1.5" style={NO_DRAG_STYLE}>
      {isClaudeDesktop ? (
        <ClaudeDesktopRouteToggle />
      ) : (
        settings?.enableLocalProxy && <ProxyToggle activeApp={activeApp} />
      )}
      {!isClaudeDesktop && settings?.enableFailoverToggle && (
        <FailoverToggle activeApp={activeApp} />
      )}
    </div>
  );
}

export function AppHeader(props: AppHeaderProps) {
  const { t } = useTranslation();
  const {
    currentView,
    onNavigate,
    activeApp,
    settings,
    dragBarHeight,
    onOpenHermesWebUI,
  } = props;
  const isHome = currentView === "providers";

  const toolbarRef = useRef<HTMLDivElement>(null);
  const isToolbarCompact = useAutoCompact(toolbarRef);

  const handleFeatureNav = (target: FeatureNavTarget) => {
    if (target === "hermesWebUI") {
      onOpenHermesWebUI();
    } else {
      onNavigate(target);
    }
  };

  return (
    <header
      className="fixed z-50 w-full transition-all duration-300 bg-background/80 backdrop-blur-md"
      {...DRAG_REGION_ATTR}
      style={{
        ...PLATFORM_DRAG_STYLE,
        top: dragBarHeight,
        height: HEADER_HEIGHT,
      }}
    >
      <div
        className="flex h-full items-center justify-between gap-2 px-6"
        {...DRAG_REGION_ATTR}
        style={PLATFORM_DRAG_STYLE}
      >
        <div className="flex items-center gap-1" style={NO_DRAG_STYLE}>
          {isHome ? (
            <BrandBar {...props} />
          ) : (
            <ViewTitleBar
              view={currentView}
              sharedFeatureApp={props.sharedFeatureApp}
              onNavigate={onNavigate}
            />
          )}
        </div>

        <div className="flex flex-1 min-w-0 items-center justify-end gap-1.5">
          {isHome && !props.activeAppCaps.additive && (
            <ProxyControls activeApp={activeApp} settings={settings} />
          )}
          {isHome && (settings?.showProfileSwitcher ?? true) && (
            <div className="flex shrink-0 items-center" style={NO_DRAG_STYLE}>
              <ProfileSwitcher activeApp={activeApp} />
            </div>
          )}
          <div
            ref={toolbarRef}
            className="flex flex-1 min-w-0 overflow-x-hidden items-center py-4 pr-2"
          >
            <div
              className="flex shrink-0 items-center gap-1.5 ml-auto"
              style={NO_DRAG_STYLE}
            >
              <HeaderViewActions
                view={currentView}
                panelRefs={props.panelRefs}
                skillsDiscoverySource={props.skillsDiscoverySource}
                onOpenSkillsDiscovery={props.onOpenSkillsDiscovery}
              />
              {isHome && (
                <>
                  <AppSwitcher
                    activeApp={activeApp}
                    onSwitch={props.onSwitchApp}
                    visibleApps={props.visibleApps}
                    compact={isToolbarCompact}
                  />
                  <AppFeatureNav
                    activeApp={activeApp}
                    caps={props.sharedFeatureCaps}
                    onNavigate={handleFeatureNav}
                  />
                  <Button
                    data-testid="add-provider"
                    aria-label={t("provider.addProvider")}
                    onClick={props.onAddProvider}
                    size="icon"
                    className={`ml-2 ${ADD_ACTION_BUTTON_CLASS}`}
                  >
                    <Plus className="w-5 h-5" />
                  </Button>
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    </header>
  );
}
