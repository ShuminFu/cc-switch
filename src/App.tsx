import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import type { Provider, VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useProviderShellActions } from "@/hooks/useProviderShellActions";
import { useOpenClawHealth } from "@/hooks/useOpenClaw";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useAppShellEvents } from "@/hooks/useAppShellEvents";
import { useEnvConflicts } from "@/hooks/useEnvConflicts";
import { useWindowChrome } from "@/hooks/useWindowChrome";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { safeStorageSet } from "@/lib/storage";
import { ALL_APP_IDS, useAppCaps } from "@/entities/app/registry";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { LazyMount, lazyNamed } from "@/components/common/LazyMount";
import type { SkillsPageSource } from "@/components/skills/skillsPageHeaderActions";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import OpenClawHealthBanner from "@/components/openclaw/OpenClawHealthBanner";
import { AppHeader } from "@/components/app-shell/AppHeader";
import { AppViewContent } from "@/components/app-shell/AppViewContent";
import { ProvidersHome } from "@/components/app-shell/ProvidersHome";
import { WindowControlsBar } from "@/components/app-shell/WindowControlsBar";
import {
  HermesLaunchConfirmDialog,
  ProviderConfirmDialog,
  useProviderConfirm,
} from "@/components/app-shell/AppConfirmDialogs";
import { useShellPanelRefs } from "@/components/app-shell/panelRefs";
import { useViewShortcuts } from "@/components/app-shell/useViewShortcuts";
import {
  VIEW_STORAGE_KEY,
  getInitialApp,
  getInitialView,
  type View,
} from "@/components/app-shell/views";

// 供应商表单（含各应用预设与 CodeMirror）与用量脚本编辑器（含 prettier）
// 体积较大，首次打开时才加载
const AddProviderDialog = lazyNamed(
  () => import("@/components/providers/AddProviderDialog"),
  "AddProviderDialog",
);
const EditProviderDialog = lazyNamed(
  () => import("@/components/providers/EditProviderDialog"),
  "EditProviderDialog",
);
const UsageScriptModal = lazy(() => import("@/components/UsageScriptModal"));

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

/** 使用这些视图时显示 OpenClaw 健康检查横幅 */
const OPENCLAW_HEALTH_VIEWS: readonly View[] = [
  "providers",
  "workspace",
  "sessions",
  "openclawEnv",
  "openclawTools",
  "openclawAgents",
];

function App() {
  const [activeApp, setActiveApp] = useState<AppId>(getInitialApp);
  const sharedFeatureApp: AppId =
    activeApp === "claude-desktop" ? "claude" : activeApp;
  const sharedFeatureCaps = useAppCaps(sharedFeatureApp);
  const activeAppCaps = useAppCaps(activeApp);
  const [currentView, setCurrentView] = useState<View>(getInitialView);
  const [skillsDiscoverySource, setSkillsDiscoverySource] =
    useState<SkillsPageSource>("repos");
  const [settingsDefaultTab, setSettingsDefaultTab] = useState("general");
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [launchDashboardOpen, setLaunchDashboardOpen] = useState(false);
  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);
  const panelRefs = useShellPanelRefs();

  useEffect(() => {
    safeStorageSet(VIEW_STORAGE_KEY, currentView);
  }, [currentView]);

  const { data: settingsData } = useSettingsQuery();
  const chrome = useWindowChrome(settingsData);
  const visibleApps = settingsData?.visibleApps ?? DEFAULT_VISIBLE_APPS;

  useEffect(() => {
    if (!visibleApps[activeApp]) {
      setActiveApp(ALL_APP_IDS.find((id) => visibleApps[id]) ?? "claude");
    }
  }, [visibleApps, activeApp]);

  // Fallback from sessions view when switching to an app without session support
  useEffect(() => {
    if (currentView === "sessions" && !sharedFeatureCaps.sessions) {
      setCurrentView("providers");
    }
  }, [sharedFeatureCaps.sessions, currentView]);

  useUsageCacheBridge();
  useViewShortcuts(currentView, setCurrentView);

  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  const isCurrentAppTakeoverActive = takeoverStatus?.[activeApp] || false;
  const activeProviderId = useMemo(() => {
    const target = proxyStatus?.active_targets?.find(
      (t) => t.app_type === activeApp,
    );
    return target?.provider_id;
  }, [proxyStatus?.active_targets, activeApp]);

  const { data, isLoading, refetch } = useProvidersQuery(activeApp, {
    isProxyRunning,
  });
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";
  const isOpenClawView =
    activeApp === "openclaw" && OPENCLAW_HEALTH_VIEWS.includes(currentView);
  const { data: openclawHealthWarnings = [] } =
    useOpenClawHealth(isOpenClawView);

  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
    switchingProviderId,
  } = useProviderActions(
    activeApp,
    isProxyRunning,
    isProxyRunning && isCurrentAppTakeoverActive,
  );
  const shellActions = useProviderShellActions({
    activeApp,
    providers,
    addProvider,
    refetchProviders: refetch,
  });
  const providerConfirm = useProviderConfirm(activeApp, deleteProvider);
  const env = useEnvConflicts(activeApp);
  useAppShellEvents(activeApp, refetch);

  const openHermesWebUI = useOpenHermesWebUI(() =>
    setLaunchDashboardOpen(true),
  );

  const openSettings = (tab: string) => {
    setSettingsDefaultTab(tab);
    setCurrentView("settings");
  };

  const openSkillsDiscovery = () => {
    setSkillsDiscoverySource("repos");
    setCurrentView("skillsDiscovery");
  };

  const providersView = (
    <ProvidersHome
      activeApp={activeApp}
      providers={providers}
      currentProviderId={currentProviderId}
      isLoading={isLoading}
      isProxyRunning={isProxyRunning}
      isProxyTakeover={isProxyRunning && isCurrentAppTakeoverActive}
      activeProviderId={activeProviderId}
      onSwitch={switchProvider}
      switchingProviderId={switchingProviderId}
      onSetAsDefaultModel={setAsDefaultModel}
      shellActions={shellActions}
      onEdit={setEditingProvider}
      onConfigureUsage={setUsageProvider}
      onCreate={() => setIsAddOpen(true)}
      onRequestConfirm={providerConfirm.request}
    />
  );

  return (
    <div
      className="flex flex-col h-screen overflow-hidden bg-background text-foreground selection:bg-primary/30 pb-4"
      style={{ overflowX: "hidden", paddingTop: chrome.contentTopOffset }}
    >
      <WindowControlsBar chrome={chrome} />
      {env.showBanner && (
        <EnvWarningBanner
          conflicts={env.conflicts}
          onDismiss={env.dismiss}
          onDeleted={env.recheck}
        />
      )}

      <AppHeader
        currentView={currentView}
        onNavigate={setCurrentView}
        onOpenSettings={openSettings}
        activeApp={activeApp}
        onSwitchApp={setActiveApp}
        visibleApps={visibleApps}
        activeAppCaps={activeAppCaps}
        sharedFeatureApp={sharedFeatureApp}
        sharedFeatureCaps={sharedFeatureCaps}
        settings={settingsData}
        isProxyRunning={isProxyRunning}
        isTakeoverActive={isCurrentAppTakeoverActive}
        dragBarHeight={chrome.dragBarHeight}
        panelRefs={panelRefs}
        skillsDiscoverySource={skillsDiscoverySource}
        onOpenSkillsDiscovery={openSkillsDiscovery}
        onOpenHermesWebUI={() => void openHermesWebUI()}
        onAddProvider={() => setIsAddOpen(true)}
      />

      <main className="flex-1 min-h-0 flex flex-col overflow-y-auto animate-fade-in">
        {isOpenClawView && openclawHealthWarnings.length > 0 && (
          <OpenClawHealthBanner warnings={openclawHealthWarnings} />
        )}
        <AppViewContent
          view={currentView}
          onNavigate={setCurrentView}
          sharedFeatureApp={sharedFeatureApp}
          panelRefs={panelRefs}
          settingsDefaultTab={settingsDefaultTab}
          onImportSuccess={shellActions.refreshAfterImport}
          onSkillsDiscoverySourceChange={setSkillsDiscoverySource}
          onOpenSkillsDiscovery={openSkillsDiscovery}
          providersView={providersView}
        />
      </main>

      <LazyMount when={isAddOpen}>
        <AddProviderDialog
          open={isAddOpen}
          onOpenChange={setIsAddOpen}
          appId={activeApp}
          onSubmit={addProvider}
        />
      </LazyMount>

      <LazyMount when={Boolean(editingProvider)}>
        <EditProviderDialog
          open={Boolean(editingProvider)}
          provider={effectiveEditingProvider}
          onOpenChange={(open) => {
            if (!open) {
              setEditingProvider(null);
            }
          }}
          onSubmit={async ({ provider, originalId }) => {
            await updateProvider(provider, originalId);
            setEditingProvider(null);
          }}
          appId={activeApp}
          isProxyTakeover={isCurrentAppTakeoverActive}
        />
      </LazyMount>

      {effectiveUsageProvider && (
        <Suspense fallback={null}>
          <UsageScriptModal
            key={effectiveUsageProvider.id}
            provider={effectiveUsageProvider}
            appId={activeApp}
            isOpen={Boolean(usageProvider)}
            onClose={() => setUsageProvider(null)}
            onSave={(script) => {
              if (usageProvider) {
                void saveUsageScript(usageProvider, script);
              }
            }}
          />
        </Suspense>
      )}

      <ProviderConfirmDialog
        pending={providerConfirm.pending}
        onConfirm={providerConfirm.confirm}
        onCancel={providerConfirm.cancel}
      />
      <HermesLaunchConfirmDialog
        isOpen={launchDashboardOpen}
        onClose={() => setLaunchDashboardOpen(false)}
      />

      <DeepLinkImportDialog />
      <FirstRunNoticeDialog />
    </div>
  );
}

export default App;
