import { lazy, Suspense, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import {
  BarChart2,
  Book,
  Brain,
  Cpu,
  FolderOpen,
  History,
  KeyRound,
  LayoutDashboard,
  Plus,
  Settings,
  Shield,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { Provider } from "@/types";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import { hermesApi } from "@/lib/api/hermes";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { liveProviderIdsKey, queryKeys } from "@/lib/query/keys";
import {
  useDisableCurrentOmo,
  useDisableCurrentOmoSlim,
} from "@/lib/query/omo";
import { APP_REGISTRY, appHas, type AppToolbarFeature } from "@/config/apps";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useDuplicateProvider } from "@/hooks/useDuplicateProvider";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { McpIcon } from "@/components/BrandIcons";
import { AppSwitcher } from "@/components/AppSwitcher";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { UpdateBadge } from "@/components/UpdateBadge";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { ProviderList } from "@/components/providers/ProviderList";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import {
  HeaderToolbar,
  NO_DRAG_STYLE,
  PageHeaderActions,
  PageHeaderStart,
} from "@/app/PageHeader";
import { useNavigation, type ViewId } from "@/app/navigation";
import {
  ProvidersErrorState,
  shouldShowProvidersError,
} from "@/app/ProvidersErrorState";

// The provider dialogs pull in the form stack (CodeMirror, presets); load
// them on first open instead of with the providers page (PERF-15).
const AddProviderDialog = lazy(() =>
  import("@/components/providers/AddProviderDialog").then((m) => ({
    default: m.AddProviderDialog,
  })),
);
const EditProviderDialog = lazy(() =>
  import("@/components/providers/EditProviderDialog").then((m) => ({
    default: m.EditProviderDialog,
  })),
);
const UsageScriptModal = lazy(() => import("@/components/UsageScriptModal"));

const ADD_BUTTON_CLASS =
  "bg-orange-500 hover:bg-orange-600 dark:bg-orange-500 dark:hover:bg-orange-600 text-white shadow-lg shadow-orange-500/30 dark:shadow-orange-500/40 rounded-full w-8 h-8";
const TOOLBAR_BUTTON_CLASS =
  "text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2";

interface ToolbarItem {
  titleKey: string;
  icon: LucideIcon | "mcp";
  /** Page to open, or a custom action id. */
  target: ViewId | "hermesWebUI";
}

/** Toolbar shortcut per registry feature id. */
const TOOLBAR_ITEMS: Record<AppToolbarFeature, ToolbarItem> = {
  skills: { titleKey: "skills.manage", icon: Wrench, target: "skills" },
  prompts: { titleKey: "prompts.manage", icon: Book, target: "prompts" },
  sessions: {
    titleKey: "sessionManager.title",
    icon: History,
    target: "sessions",
  },
  mcp: { titleKey: "mcp.title", icon: "mcp", target: "mcp" },
  workspace: {
    titleKey: "workspace.manage",
    icon: FolderOpen,
    target: "workspace",
  },
  openclawEnv: {
    titleKey: "openclaw.env.title",
    icon: KeyRound,
    target: "openclawEnv",
  },
  openclawTools: {
    titleKey: "openclaw.tools.title",
    icon: Shield,
    target: "openclawTools",
  },
  openclawAgents: {
    titleKey: "openclaw.agents.title",
    icon: Cpu,
    target: "openclawAgents",
  },
  hermesMemory: {
    titleKey: "hermes.memory.title",
    icon: Brain,
    target: "hermesMemory",
  },
  hermesWebUI: {
    titleKey: "hermes.webui.open",
    icon: LayoutDashboard,
    target: "hermesWebUI",
  },
};

/** Root page: the provider list of the active app. */
export default function ProvidersPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { activeApp, setActiveApp, navigate } = useNavigation();
  const app = APP_REGISTRY[activeApp];

  const { data: settingsData } = useSettingsQuery();
  const visibleApps = settingsData?.visibleApps;

  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  const isCurrentAppTakeoverActive =
    (takeoverStatus as Partial<Record<AppId, boolean>> | undefined)?.[
      activeApp
    ] || false;
  const activeProviderId = useMemo(() => {
    const target = proxyStatus?.active_targets?.find(
      (target) => target.app_type === activeApp,
    );
    return target?.provider_id;
  }, [proxyStatus?.active_targets, activeApp]);

  const providersQuery = useProvidersQuery(activeApp, { isProxyRunning });
  const { data, isLoading, refetch } = providersQuery;
  const showProvidersError = shouldShowProvidersError(providersQuery);
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";

  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
    isLoading: isProviderActionPending,
  } = useProviderActions(
    activeApp,
    isProxyRunning,
    isProxyRunning && isCurrentAppTakeoverActive,
  );

  const [isAddOpen, setIsAddOpen] = useState(false);
  const [addDialogLoaded, setAddDialogLoaded] = useState(false);
  const openAddDialog = () => {
    setAddDialogLoaded(true);
    setIsAddOpen(true);
  };
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    provider: Provider;
    action: "remove" | "delete";
  } | null>(null);
  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);

  const [launchDashboardOpen, setLaunchDashboardOpen] = useState(false);
  const openHermesWebUI = useOpenHermesWebUI(() =>
    setLaunchDashboardOpen(true),
  );

  const disableOmoMutation = useDisableCurrentOmo();
  const disableOmoSlimMutation = useDisableCurrentOmoSlim();
  const omoCallbacks = {
    onSuccess: () => {
      toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" }));
    },
    onError: (error: unknown) => {
      toast.error(
        t("omo.disableFailed", {
          defaultValue: "停用 OMO 失败: {{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  };
  const handleDisableOmo = () =>
    disableOmoMutation.mutate(undefined, omoCallbacks);
  const handleDisableOmoSlim = () =>
    disableOmoSlimMutation.mutate(undefined, omoCallbacks);

  const handleOpenWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("notifications.openLinkFailed", {
          defaultValue: "链接打开失败",
        });
      toast.error(detail);
    }
  };

  const handleEditProvider = async ({
    provider,
    originalId,
  }: {
    provider: Provider;
    originalId?: string;
  }) => {
    await updateProvider(provider, originalId);
    setEditingProvider(null);
  };

  // PRODUCT-73: one confirm at a time; errors always surface and the dialog
  // always closes, so repeated clicks cannot re-issue the command.
  const confirmPendingRef = useRef(false);
  const [confirmPending, setConfirmPending] = useState(false);
  const handleConfirmAction = async () => {
    if (!confirmAction || confirmPendingRef.current) return;
    const { provider, action } = confirmAction;
    confirmPendingRef.current = true;
    setConfirmPending(true);

    try {
      if (action === "remove") {
        // Remove from live config only (additive-mode apps); the provider
        // stays in the database and the list.
        await providersApi.removeFromLiveConfig(provider.id, activeApp);
        const liveKey = liveProviderIdsKey(activeApp);
        await Promise.all([
          liveKey ? queryClient.invalidateQueries({ queryKey: liveKey }) : null,
          appHas(activeApp, "healthBanner")
            ? queryClient.invalidateQueries({
                queryKey: queryKeys.openclaw.health,
              })
            : null,
        ]);
        toast.success(
          t("notifications.removeFromConfigSuccess", {
            defaultValue: "已从配置移除",
          }),
          { closeButton: true },
        );
      } else {
        await deleteProvider(provider.id);
      }
    } catch (error) {
      // The delete mutation already shows its own error toast.
      if (action === "remove") {
        toast.error(
          t("notifications.removeFromConfigFailed", {
            defaultValue: "Failed to remove from config: {{error}}",
            error: extractErrorMessage(error) || String(error),
          }),
        );
      }
    } finally {
      confirmPendingRef.current = false;
      setConfirmPending(false);
      setConfirmAction(null);
    }
  };

  const handleDuplicateProvider = useDuplicateProvider(
    activeApp,
    providers,
    addProvider,
  );

  const handleOpenTerminal = async (provider: Provider) => {
    try {
      const selectedDir = await settingsApi.pickDirectory();
      if (!selectedDir) {
        return;
      }

      await providersApi.openTerminal(provider.id, activeApp, {
        cwd: selectedDir,
      });
      toast.success(
        t("provider.terminalOpened", {
          defaultValue: "终端已打开",
        }),
      );
    } catch (error) {
      console.error("[App] Failed to open terminal", error);
      const errorMessage = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed", {
          defaultValue: "打开终端失败",
        }) + (errorMessage ? `: ${errorMessage}` : ""),
      );
    }
  };

  const runToolbarItem = (item: ToolbarItem) => {
    if (item.target === "hermesWebUI") {
      void openHermesWebUI();
    } else {
      navigate(item.target);
    }
  };

  const showProxyControls = app.capabilities.headerProxyControls;

  return (
    <>
      <PageHeaderStart>
        <div className="flex items-center gap-2">
          <div className="relative inline-flex items-center">
            <a
              href="https://ccswitch.io"
              target="_blank"
              rel="noreferrer"
              className={cn(
                "text-xl font-semibold transition-colors",
                isProxyRunning && isCurrentAppTakeoverActive
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
            onClick={() => navigate("settings", { tab: "general" })}
            title={t("common.settings")}
            aria-label={t("common.settings")}
            className="hover:bg-black/5 dark:hover:bg-white/5"
          >
            <Settings className="w-4 h-4" />
          </Button>
          <UpdateBadge onClick={() => navigate("settings", { tab: "about" })} />
          {isCurrentAppTakeoverActive && (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => navigate("settings", { tab: "usage" })}
              title={t("usage.title", { defaultValue: "使用统计" })}
              aria-label={t("usage.title", { defaultValue: "使用统计" })}
              className="hover:bg-black/5 dark:hover:bg-white/5"
            >
              <BarChart2 className="w-4 h-4" />
            </Button>
          )}
        </div>
      </PageHeaderStart>

      <PageHeaderActions>
        {showProxyControls && (
          <div
            className="flex shrink-0 items-center gap-1.5"
            style={NO_DRAG_STYLE}
          >
            {app.capabilities.desktopRoute ? (
              <ClaudeDesktopRouteToggle />
            ) : (
              app.capabilities.proxy &&
              settingsData?.enableLocalProxy && (
                <ProxyToggle activeApp={activeApp} />
              )
            )}
            {app.capabilities.failover &&
              settingsData?.enableFailoverToggle && (
                <FailoverToggle activeApp={activeApp} />
              )}
          </div>
        )}
        {(settingsData?.showProfileSwitcher ?? true) && (
          <div className="flex shrink-0 items-center" style={NO_DRAG_STYLE}>
            <ProfileSwitcher activeApp={activeApp} />
          </div>
        )}
        <HeaderToolbar>
          {(compact) => (
            <>
              <AppSwitcher
                activeApp={activeApp}
                onSwitch={setActiveApp}
                visibleApps={visibleApps}
                compact={compact}
              />

              <div className="flex items-center gap-1 p-1 bg-muted rounded-xl">
                <AnimatePresence mode="wait">
                  <motion.div
                    key={app.toolbar.join(",")}
                    className="flex items-center gap-1"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.15 }}
                  >
                    {app.toolbar.map((feature) => {
                      const item = TOOLBAR_ITEMS[feature];
                      const Icon = item.icon;
                      return (
                        <Button
                          key={feature}
                          variant="ghost"
                          size="sm"
                          onClick={() => runToolbarItem(item)}
                          className={TOOLBAR_BUTTON_CLASS}
                          title={t(item.titleKey)}
                          aria-label={t(item.titleKey)}
                        >
                          {Icon === "mcp" ? (
                            <McpIcon size={16} />
                          ) : (
                            <Icon className="flex-shrink-0 w-4 h-4" />
                          )}
                        </Button>
                      );
                    })}
                  </motion.div>
                </AnimatePresence>
              </div>

              <Button
                onClick={openAddDialog}
                size="icon"
                className={`ml-2 ${ADD_BUTTON_CLASS}`}
                title={t("provider.addProvider", {
                  defaultValue: "Add provider",
                })}
                aria-label={t("provider.addProvider", {
                  defaultValue: "Add provider",
                })}
              >
                <Plus className="w-5 h-5" />
              </Button>
            </>
          )}
        </HeaderToolbar>
      </PageHeaderActions>

      <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
        <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
          <AnimatePresence mode="wait">
            <motion.div
              key={activeApp}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="space-y-4"
            >
              {showProvidersError ? (
                <ProvidersErrorState
                  error={providersQuery.error}
                  onRetry={() => void refetch()}
                  isRetrying={providersQuery.isFetching}
                />
              ) : (
                <ProviderList
                  providers={providers}
                  currentProviderId={currentProviderId}
                  appId={activeApp}
                  isLoading={isLoading}
                  isActionPending={isProviderActionPending}
                  isProxyRunning={isProxyRunning}
                  isProxyTakeover={isProxyRunning && isCurrentAppTakeoverActive}
                  activeProviderId={activeProviderId}
                  onSwitch={switchProvider}
                  onEdit={setEditingProvider}
                  onDelete={(provider) =>
                    setConfirmAction({ provider, action: "delete" })
                  }
                  onRemoveFromConfig={
                    app.capabilities.additiveLiveConfig
                      ? (provider) =>
                          setConfirmAction({ provider, action: "remove" })
                      : undefined
                  }
                  onDisableOmo={
                    app.capabilities.omo ? handleDisableOmo : undefined
                  }
                  onDisableOmoSlim={
                    app.capabilities.omo ? handleDisableOmoSlim : undefined
                  }
                  onDuplicate={handleDuplicateProvider}
                  onConfigureUsage={setUsageProvider}
                  onOpenWebsite={handleOpenWebsite}
                  onOpenTerminal={
                    app.capabilities.terminal ? handleOpenTerminal : undefined
                  }
                  onCreate={openAddDialog}
                  onSetAsDefault={
                    app.setAsDefault === "model"
                      ? setAsDefaultModel
                      : app.setAsDefault === "switch"
                        ? switchProvider
                        : undefined
                  }
                />
              )}
            </motion.div>
          </AnimatePresence>
        </div>
      </div>

      <Suspense fallback={null}>
        {addDialogLoaded && (
          <AddProviderDialog
            open={isAddOpen}
            onOpenChange={setIsAddOpen}
            appId={activeApp}
            onSubmit={addProvider}
          />
        )}

        {effectiveEditingProvider && (
          <EditProviderDialog
            open={Boolean(editingProvider)}
            provider={effectiveEditingProvider}
            onOpenChange={(open) => {
              if (!open) {
                setEditingProvider(null);
              }
            }}
            onSubmit={handleEditProvider}
            appId={activeApp}
            isProxyTakeover={isCurrentAppTakeoverActive}
          />
        )}

        {effectiveUsageProvider && (
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
        )}
      </Suspense>

      <ConfirmDialog
        isOpen={Boolean(confirmAction)}
        title={
          confirmAction?.action === "remove"
            ? t("confirm.removeProvider")
            : t("confirm.deleteProvider")
        }
        message={
          confirmAction
            ? confirmAction.action === "remove"
              ? t("confirm.removeProviderMessage", {
                  name: confirmAction.provider.name,
                })
              : t("confirm.deleteProviderMessage", {
                  name: confirmAction.provider.name,
                })
            : ""
        }
        isPending={confirmPending}
        onConfirm={() => void handleConfirmAction()}
        onCancel={() => setConfirmAction(null)}
      />

      <ConfirmDialog
        isOpen={launchDashboardOpen}
        title={t("hermes.webui.launchConfirmTitle")}
        message={t("hermes.webui.launchConfirmMessage")}
        confirmText={t("hermes.webui.launchConfirmAction")}
        variant="info"
        onConfirm={() => {
          setLaunchDashboardOpen(false);
          void (async () => {
            try {
              await hermesApi.launchDashboard();
              toast.success(t("hermes.webui.launching"));
            } catch (error) {
              toast.error(t("hermes.webui.launchFailed"), {
                description: extractErrorMessage(error) || undefined,
              });
            }
          })();
        }}
        onCancel={() => setLaunchDashboardOpen(false)}
      />
    </>
  );
}
