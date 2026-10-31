import { CSS } from "@dnd-kit/utilities";
import { queryKeys } from "@/lib/query/keys";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  memo,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertTriangle, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { ProviderHealth } from "@/types/proxy";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import {
  useOpenClawLiveProviderIds,
  useOpenClawDefaultModel,
} from "@/hooks/useOpenClaw";
import {
  useHermesLiveProviderIds,
  useHermesModelConfig,
} from "@/hooks/useHermes";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { useStableCallback } from "@/hooks/useStableCallback";
import { useShortcut } from "@/lib/shortcuts";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { ProviderHealthPanel } from "@/components/providers/ProviderHealthPanel";
import {
  ImportCandidatesDialog,
  supportsImportPicker,
} from "@/components/providers/ImportCandidatesDialog";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useProvidersHealth,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import {
  useCurrentOmoProviderId,
  useCurrentOmoSlimProviderId,
} from "@/lib/query/omo";
import { useCallback } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { isTextEditableTarget } from "@/utils/domUtils";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getProviderEndpoint, getProviderModels } from "@/lib/providerCodecs";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  // A provider mutation (switch/add/update/delete) is in flight: card main
  // buttons are disabled so double clicks cannot start concurrent switches.
  isActionPending?: boolean;
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
  onSetAsDefault?: (provider: Provider) => void; // OpenClaw: set as default model
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onCreate,
  isLoading = false,
  isActionPending = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
  onSetAsDefault,
}: ProviderListProps) {
  const { t } = useTranslation();
  // Stable identities for every handler handed to the memoised cards, so a
  // parent re-render (proxy-status polls, query refetches) does not re-render
  // every card (PERF-20, FE-ARCH-09).
  const stableOnSwitch = useStableCallback(onSwitch);
  const stableOnEdit = useStableCallback(onEdit);
  const stableOnDelete = useStableCallback(onDelete);
  const stableOnRemoveFromConfig = useStableCallback(onRemoveFromConfig);
  const stableOnDisableOmo = useStableCallback(onDisableOmo);
  const stableOnDisableOmoSlim = useStableCallback(onDisableOmoSlim);
  const stableOnDuplicate = useStableCallback(onDuplicate);
  const stableOnConfigureUsage = useStableCallback(onConfigureUsage);
  const stableOnOpenWebsite = useStableCallback(onOpenWebsite);
  const stableOnOpenTerminal = useStableCallback(onOpenTerminal);
  const stableOnSetAsDefault = useStableCallback(onSetAsDefault);
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const { sortedProviders, sensors, handleDragEnd, reorder } = useDragSort(
    providers,
    appId,
  );

  const { data: opencodeLiveIds, isLoading: isOpencodeLiveIdsLoading } =
    useQuery({
      queryKey: queryKeys.providers.opencodeLiveIds,
      queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
      enabled: appId === "opencode",
    });

  // OpenClaw: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: openclawLiveIds, isLoading: isOpenclawLiveIdsLoading } =
    useOpenClawLiveProviderIds(appId === "openclaw");

  // Hermes: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: hermesLiveIds, isLoading: isHermesLiveIdsLoading } =
    useHermesLiveProviderIds(appId === "hermes");

  // Until the live ids arrive, "in config" is unknown: additive cards show a
  // disabled pending button instead of flashing "Add" (FE-PROV-51).
  const isConfigStatusLoading =
    (appId === "opencode" && isOpencodeLiveIdsLoading) ||
    (appId === "openclaw" && isOpenclawLiveIdsLoading) ||
    (appId === "hermes" && isHermesLiveIdsLoading);

  // Hermes: 读取当前 model.provider，用于判断哪个供应商是"当前激活"（高亮）
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const hermesCurrentProviderId = hermesModelConfig?.provider;

  // 判断供应商是否已添加到配置（累加模式应用：OpenCode/OpenClaw/Hermes）
  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId === "opencode") {
        return opencodeLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "openclaw") {
        return openclawLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "hermes") {
        return hermesLiveIds?.includes(providerId) ?? false;
      }
      return true; // 其他应用始终返回 true
    },
    [appId, opencodeLiveIds, openclawLiveIds, hermesLiveIds],
  );

  // OpenClaw: query default model to determine which provider is default
  const { data: openclawDefaultModel } = useOpenClawDefaultModel(
    appId === "openclaw",
  );

  const isProviderDefaultModel = useCallback(
    (providerId: string): boolean => {
      if (appId !== "openclaw" || !openclawDefaultModel?.primary) return false;
      return openclawDefaultModel.primary.startsWith(providerId + "/");
    },
    [appId, openclawDefaultModel],
  );

  // 故障转移相关
  const { data: isAutoFailoverEnabled } = useAutoFailoverEnabled(appId);
  const { data: failoverQueue } = useFailoverQueue(appId);
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();

  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;

  const isOpenCode = appId === "opencode";
  const { data: currentOmoId } = useCurrentOmoProviderId(isOpenCode);
  const { data: currentOmoSlimId } = useCurrentOmoSlimProviderId(isOpenCode);

  const getFailoverPriority = useCallback(
    (providerId: string): number | undefined => {
      if (!isFailoverModeActive || !failoverQueue) return undefined;
      const index = failoverQueue.findIndex(
        (item) => item.providerId === providerId,
      );
      return index >= 0 ? index + 1 : undefined;
    },
    [isFailoverModeActive, failoverQueue],
  );

  const isInFailoverQueue = useCallback(
    (providerId: string): boolean => {
      if (!isFailoverModeActive || !failoverQueue) return false;
      return failoverQueue.some((item) => item.providerId === providerId);
    },
    [isFailoverModeActive, failoverQueue],
  );

  // One batched health poll for the queued providers instead of one per card
  // (FE-ARCH-09); the badge only renders for queued cards while proxying.
  const queuedProviderIds = useMemo(
    () =>
      isFailoverModeActive && failoverQueue
        ? failoverQueue.map((item) => item.providerId)
        : EMPTY_IDS,
    [isFailoverModeActive, failoverQueue],
  );
  const healthById = useProvidersHealth(
    appId,
    queuedProviderIds,
    isProxyRunning,
  );

  const isFailoverMutationPending =
    addToQueue.isPending || removeFromQueue.isPending;
  const isAnyActionPending = isActionPending || isFailoverMutationPending;

  const handleToggleFailover = useStableCallback(
    (providerId: string, enabled: boolean) => {
      // Ignore repeated clicks while a queue mutation is still running.
      if (addToQueue.isPending || removeFromQueue.isPending) return;
      if (enabled) {
        addToQueue.mutate({ appType: appId, providerId });
      } else {
        removeFromQueue.mutate({ appType: appId, providerId });
      }
    },
  );

  const [searchTerm, setSearchTerm] = useState("");
  const [isImportPickerOpen, setIsImportPickerOpen] = useState(false);
  const [isHealthOpen, setIsHealthOpen] = useState(false);
  const canPickImport = supportsImportPicker(appId);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const { data: claudeDesktopStatus } = useQuery({
    queryKey: queryKeys.providers.claudeDesktopStatus,
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    enabled: appId === "claude-desktop",
    refetchInterval: appId === "claude-desktop" ? 5000 : false,
  });

  // 连通性检查不发真实请求、无封号/计费风险，直接执行（无需确认弹窗）。
  const handleTest = useStableCallback((provider: Provider) => {
    checkProvider(provider.id, provider.name);
  });

  // Import current live config as default provider
  const queryClient = useQueryClient();
  const importMutation = useMutation({
    mutationFn: async (): Promise<boolean> => {
      if (appId === "opencode") {
        const count = await providersApi.importOpenCodeFromLive();
        return count > 0;
      }
      if (appId === "openclaw") {
        const count = await providersApi.importOpenClawFromLive();
        return count > 0;
      }
      if (appId === "hermes") {
        const count = await providersApi.importHermesFromLive();
        return count > 0;
      }
      if (appId === "claude-desktop") {
        const count = await providersApi.importClaudeDesktopFromClaude();
        return count > 0;
      }
      return providersApi.importDefault(appId);
    },
    onSuccess: (imported) => {
      if (imported) {
        queryClient.invalidateQueries({
          queryKey: queryKeys.providers.byApp(appId),
        });
        if (appId === "claude-desktop") {
          queryClient.invalidateQueries({
            queryKey: queryKeys.providers.claudeDesktopStatus,
          });
        }
        toast.success(t("provider.importCurrentDescription"));
      } else {
        toast.info(t("provider.noProviders"));
      }
    },
    onError: (error: unknown) => {
      toast.error(extractErrorMessage(error) || t("common.unknown"));
    },
  });

  // Cmd/Ctrl+F opens the search, Escape closes it (shortcut registry,
  // FE-ARCH-16). Ctrl+F is left to text fields such as the preset search.
  useShortcut(
    {
      id: "providers.search",
      keys: "mod+f",
      label: t("provider.searchShortcut", {
        defaultValue: "Search providers",
      }),
      allowInInput: true,
    },
    () => {
      if (isTextEditableTarget(document.activeElement)) return false;
      setIsSearchOpen(true);
    },
  );
  useShortcut(
    {
      id: "providers.search.close",
      keys: "escape",
      label: t("provider.searchCloseShortcut", {
        defaultValue: "Close provider search",
      }),
      allowInInput: true,
    },
    () => {
      setIsSearchOpen(false);
      // Let other Escape handlers (dialogs, back navigation) run too.
      return false;
    },
    isSearchOpen,
  );

  useEffect(() => {
    if (isSearchOpen) {
      const frame = requestAnimationFrame(() => {
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [isSearchOpen]);

  // Filter chips (FE-PROV-33): by category, and "in failover queue" while
  // failover is active. Shown only for lists long enough to need them.
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null);
  const [queueOnly, setQueueOnly] = useState(false);
  const presentCategories = useMemo(() => {
    const seen = new Set<string>();
    for (const provider of sortedProviders) {
      if (provider.category) seen.add(provider.category);
    }
    return CATEGORY_FILTER_ORDER.filter((category) => seen.has(category));
  }, [sortedProviders]);
  const showFilterChips =
    sortedProviders.length >= FILTER_CHIPS_MIN_PROVIDERS &&
    (presentCategories.length > 1 || isFailoverModeActive);
  const activeCategoryFilter =
    showFilterChips &&
    categoryFilter &&
    presentCategories.includes(categoryFilter)
      ? categoryFilter
      : null;
  const activeQueueOnly = showFilterChips && isFailoverModeActive && queueOnly;

  const filteredProviders = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    const chipFiltered =
      activeCategoryFilter || activeQueueOnly
        ? sortedProviders.filter(
            (provider) =>
              (!activeCategoryFilter ||
                provider.category === activeCategoryFilter) &&
              (!activeQueueOnly || isInFailoverQueue(provider.id)),
          )
        : sortedProviders;
    if (!keyword) return chipFiltered;
    return chipFiltered.filter((provider) => {
      // Endpoint and configured models are searchable too (FE-PROV-54).
      const fields = [
        provider.name,
        provider.notes,
        provider.websiteUrl,
        getProviderEndpoint(appId, provider),
        ...getProviderModels(appId, provider),
      ];
      return fields.some((field) =>
        field?.toString().toLowerCase().includes(keyword),
      );
    });
  }, [
    appId,
    searchTerm,
    sortedProviders,
    activeCategoryFilter,
    activeQueueOnly,
    isInFailoverQueue,
  ]);

  const sortableItems = useMemo(
    () => filteredProviders.map((provider) => provider.id),
    [filteredProviders],
  );

  // Long lists (aggregator users with dozens of providers) are virtualised so
  // only the visible cards mount (FE-ARCH-09). The scroll container is the
  // nearest scrollable ancestor of the list.
  const listRef = useRef<HTMLDivElement>(null);
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);
  const [scrollMargin, setScrollMargin] = useState(0);
  const shouldVirtualize = filteredProviders.length > VIRTUALIZE_THRESHOLD;
  useLayoutEffect(() => {
    if (!shouldVirtualize || !listRef.current) return;
    const scroller = findScrollParent(listRef.current);
    if (scroller !== scrollElement) setScrollElement(scroller);
    const margin =
      listRef.current.getBoundingClientRect().top -
      scroller.getBoundingClientRect().top +
      scroller.scrollTop;
    if (Math.abs(margin - scrollMargin) > 1) setScrollMargin(margin);
  }, [shouldVirtualize, scrollElement, scrollMargin, filteredProviders]);
  const virtualizer = useVirtualizer({
    count: shouldVirtualize ? filteredProviders.length : 0,
    getScrollElement: () => scrollElement,
    estimateSize: () => ESTIMATED_CARD_HEIGHT,
    overscan: 6,
    scrollMargin,
    getItemKey: (index) => filteredProviders[index]?.id ?? index,
    initialRect: { width: 800, height: 800 },
  });

  const claudeDesktopStatusMessages = useMemo(() => {
    if (appId !== "claude-desktop" || !claudeDesktopStatus) return [];

    const messages: string[] = [];
    if (!claudeDesktopStatus.supported) {
      messages.push(
        t("claudeDesktop.statusUnsupported", {
          defaultValue:
            "This platform does not support writing Claude Desktop 3P configuration yet.",
        }),
      );
      return messages;
    }

    if (claudeDesktopStatus.staleRawModels) {
      messages.push(
        t("claudeDesktop.statusStaleRawModels", {
          defaultValue:
            "The Claude Desktop profile contains model IDs outside the Sonnet / Opus / Haiku role families. Newer Claude Desktop versions may reject it; switch to the current provider again to repair it.",
        }),
      );
    }
    if (claudeDesktopStatus.missingRouteMappings) {
      messages.push(
        t("claudeDesktop.statusMissingRouteMappings", {
          defaultValue:
            "The current provider has model mapping enabled but no valid routes. Edit the provider and add at least one mapping.",
        }),
      );
    }
    if (
      claudeDesktopStatus.mode === "proxy" &&
      !claudeDesktopStatus.gatewayTokenConfigured
    ) {
      messages.push(
        t("claudeDesktop.statusGatewayTokenMissing", {
          defaultValue:
            "The local routing token has not been generated yet. Switching to this provider again will write a new local token.",
        }),
      );
    }

    const expected = claudeDesktopStatus.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = claudeDesktopStatus.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      messages.push(
        t("claudeDesktop.statusBaseUrlMismatch", {
          expected,
          actual,
          defaultValue:
            "The Claude Desktop profile points to a different URL than the current provider. Current: {{actual}}; expected: {{expected}}. Switch to the current provider again to repair it.",
        }),
      );
    }

    return messages;
  }, [appId, claudeDesktopStatus, t]);

  if (isLoading) {
    return (
      <div className="space-y-3">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="w-full border border-dashed rounded-lg h-28 border-muted-foreground/40 bg-muted/40"
          />
        ))}
      </div>
    );
  }

  const importPicker = canPickImport && isImportPickerOpen && (
    <ImportCandidatesDialog
      open={isImportPickerOpen}
      appId={appId}
      existingIds={Object.keys(providers)}
      onOpenChange={setIsImportPickerOpen}
    />
  );

  if (sortedProviders.length === 0) {
    return (
      <>
        <ProviderEmptyState
          appId={appId}
          onCreate={onCreate}
          onImport={() => importMutation.mutate()}
          onPickImport={
            canPickImport ? () => setIsImportPickerOpen(true) : undefined
          }
        />
        {importPicker}
      </>
    );
  }

  const renderCard = (provider: Provider) => {
    const isOmo = provider.category === "omo";
    const isOmoSlim = provider.category === "omo-slim";
    const isOmoCurrent = isOmo && provider.id === (currentOmoId || "");
    const isOmoSlimCurrent =
      isOmoSlim && provider.id === (currentOmoSlimId || "");
    const isHermesCurrent =
      appId === "hermes" && hermesCurrentProviderId === provider.id;
    return (
      <SortableProviderCard
        key={provider.id}
        provider={provider}
        isCurrent={
          isOmo
            ? isOmoCurrent
            : isOmoSlim
              ? isOmoSlimCurrent
              : appId === "hermes"
                ? isHermesCurrent
                : provider.id === currentProviderId
        }
        appId={appId}
        isInConfig={isProviderInConfig(provider.id)}
        isConfigStatusLoading={isConfigStatusLoading}
        isOmo={isOmo}
        isOmoSlim={isOmoSlim}
        onSwitch={stableOnSwitch}
        onEdit={stableOnEdit}
        onDelete={stableOnDelete}
        onRemoveFromConfig={stableOnRemoveFromConfig}
        onDisableOmo={stableOnDisableOmo}
        onDisableOmoSlim={stableOnDisableOmoSlim}
        onDuplicate={stableOnDuplicate}
        onConfigureUsage={stableOnConfigureUsage}
        onOpenWebsite={stableOnOpenWebsite}
        onOpenTerminal={stableOnOpenTerminal}
        onTest={handleTest}
        isTesting={isChecking(provider.id)}
        isActionPending={isAnyActionPending}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        isAutoFailoverEnabled={isFailoverModeActive}
        failoverPriority={getFailoverPriority(provider.id)}
        isInFailoverQueue={isInFailoverQueue(provider.id)}
        health={healthById.get(provider.id)}
        onToggleFailover={handleToggleFailover}
        activeProviderId={activeProviderId}
        // OpenClaw: default model / Hermes: model.provider === provider.id
        isDefaultModel={
          appId === "hermes"
            ? isHermesCurrent
            : isProviderDefaultModel(provider.id)
        }
        onSetAsDefault={stableOnSetAsDefault}
      />
    );
  };

  // PRODUCT-28: Alt+Up / Alt+Down moves the focused card among the visible
  // ones, the keyboard alternative to drag-and-drop.
  const handleReorderKey = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown"))
      return;
    const card = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-provider-id]",
    );
    const id = card?.dataset.providerId;
    if (!id) return;
    const index = filteredProviders.findIndex((provider) => provider.id === id);
    const target =
      filteredProviders[index + (event.key === "ArrowUp" ? -1 : 1)];
    if (index === -1 || !target) return;
    event.preventDefault();
    void reorder(id, target.id);
  };

  const renderProviderList = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={sortableItems}
        strategy={verticalListSortingStrategy}
      >
        {shouldVirtualize ? (
          <div
            ref={listRef}
            onKeyDown={handleReorderKey}
            data-testid="provider-list-virtual"
            style={{
              height: virtualizer.getTotalSize(),
              position: "relative",
            }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const provider = filteredProviders[item.index];
              if (!provider) return null;
              return (
                <div
                  key={item.key}
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  className="absolute left-0 top-0 w-full pb-3"
                  style={{
                    transform: `translateY(${item.start - virtualizer.options.scrollMargin}px)`,
                  }}
                >
                  {renderCard(provider)}
                </div>
              );
            })}
          </div>
        ) : (
          <div ref={listRef} className="space-y-3" onKeyDown={handleReorderKey}>
            {filteredProviders.map(renderCard)}
          </div>
        )}
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="mt-4 space-y-4">
      {claudeDesktopStatusMessages.length > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            {t("claudeDesktop.statusTitle", {
              defaultValue: "Claude Desktop configuration needs attention",
            })}
          </div>
          <ul className="mt-2 space-y-1 text-xs leading-relaxed">
            {claudeDesktopStatusMessages.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}
      <AnimatePresence>
        {isSearchOpen && (
          <motion.div
            key="provider-search"
            initial={{ opacity: 0, y: -8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="fixed left-1/2 top-[6.5rem] z-40 w-[min(90vw,26rem)] -translate-x-1/2 sm:right-6 sm:left-auto sm:translate-x-0"
          >
            <div className="p-4 space-y-3 border shadow-md rounded-2xl border-white/10 bg-background/95 shadow-black/20 backdrop-blur-md">
              <div className="relative flex items-center gap-2">
                <Search className="absolute w-4 h-4 -translate-y-1/2 pointer-events-none left-3 top-1/2 text-muted-foreground" />
                <Input
                  ref={searchInputRef}
                  value={searchTerm}
                  onChange={(event) => setSearchTerm(event.target.value)}
                  placeholder={t("provider.searchPlaceholder", {
                    defaultValue: "Search name, notes, or URL...",
                  })}
                  aria-label={t("provider.searchAriaLabel", {
                    defaultValue: "Search providers",
                  })}
                  className="pr-16 pl-9"
                />
                {searchTerm && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="absolute text-xs -translate-y-1/2 right-11 top-1/2"
                    onClick={() => setSearchTerm("")}
                  >
                    {t("common.clear", { defaultValue: "Clear" })}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="ml-auto"
                  onClick={() => setIsSearchOpen(false)}
                  aria-label={t("provider.searchCloseAriaLabel", {
                    defaultValue: "Close provider search",
                  })}
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                <span>
                  {t("provider.searchScopeHint", {
                    defaultValue: "Matches provider name, notes, and URL.",
                  })}
                </span>
                <span>
                  {t("provider.searchCloseHint", {
                    defaultValue: "Press Esc to close",
                  })}
                </span>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {showFilterChips && (
        <div
          className="flex flex-wrap items-center gap-1.5"
          role="toolbar"
          aria-label={t("provider.filter.label", {
            defaultValue: "Filter providers",
          })}
        >
          <FilterChip
            active={!activeCategoryFilter && !activeQueueOnly}
            onClick={() => {
              setCategoryFilter(null);
              setQueueOnly(false);
            }}
          >
            {t("provider.filter.all", { defaultValue: "All" })}
          </FilterChip>
          {presentCategories.length > 1 &&
            presentCategories.map((category) => (
              <FilterChip
                key={category}
                active={activeCategoryFilter === category}
                onClick={() =>
                  setCategoryFilter((current) =>
                    current === category ? null : category,
                  )
                }
              >
                {t(`provider.filter.category.${category}`, {
                  defaultValue: CATEGORY_FILTER_LABELS[category] ?? category,
                })}
              </FilterChip>
            ))}
          {isFailoverModeActive && (
            <FilterChip
              active={activeQueueOnly}
              onClick={() => setQueueOnly((current) => !current)}
            >
              {t("provider.filter.inQueue", {
                defaultValue: "In failover queue",
              })}
            </FilterChip>
          )}
        </div>
      )}

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : (
        renderProviderList()
      )}

      <div className="flex flex-wrap justify-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          className="text-xs text-muted-foreground"
          onClick={() => setIsHealthOpen(true)}
        >
          {t("provider.health.open", { defaultValue: "Provider health…" })}
        </Button>
        {canPickImport && (
          <Button
            variant="ghost"
            size="sm"
            className="text-xs text-muted-foreground"
            onClick={() => setIsImportPickerOpen(true)}
          >
            {t("provider.importPicker.open", {
              defaultValue: "Choose providers from the live config…",
            })}
          </Button>
        )}
      </div>
      {importPicker}
      {isHealthOpen && (
        <ProviderHealthPanel
          open={isHealthOpen}
          appId={appId}
          providers={sortedProviders}
          onOpenChange={setIsHealthOpen}
        />
      )}
    </div>
  );
}

const EMPTY_IDS: string[] = [];
/** Filter chips appear from this many providers on (FE-PROV-33). */
export const FILTER_CHIPS_MIN_PROVIDERS = 6;
const CATEGORY_FILTER_ORDER = [
  "official",
  "cn_official",
  "cloud_provider",
  "aggregator",
  "third_party",
  "custom",
  "omo",
  "omo-slim",
];
const CATEGORY_FILTER_LABELS: Record<string, string> = {
  official: "Official",
  cn_official: "Open-source official",
  cloud_provider: "Cloud",
  aggregator: "Aggregator",
  third_party: "Third party",
  custom: "Custom",
  omo: "OMO",
  "omo-slim": "OMO Slim",
};

function FilterChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={
        active
          ? "rounded-full border border-primary bg-primary/10 px-2.5 py-0.5 text-xs text-primary"
          : "rounded-full border border-border px-2.5 py-0.5 text-xs text-muted-foreground hover:bg-muted"
      }
    >
      {children}
    </button>
  );
}
/** Lists longer than this are virtualised. */
export const VIRTUALIZE_THRESHOLD = 30;
const ESTIMATED_CARD_HEIGHT = 112;

function findScrollParent(element: HTMLElement): HTMLElement {
  let node = element.parentElement;
  while (node) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY === "auto" || overflowY === "scroll") return node;
    node = node.parentElement;
  }
  return (document.scrollingElement as HTMLElement) ?? document.documentElement;
}

interface SortableProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig: boolean;
  isConfigStatusLoading?: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting: boolean;
  isActionPending?: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  isAutoFailoverEnabled: boolean;
  failoverPriority?: number;
  isInFailoverQueue: boolean;
  health?: ProviderHealth;
  onToggleFailover: (providerId: string, enabled: boolean) => void;
  activeProviderId?: string;
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: (provider: Provider) => void;
}

const noopConfigureUsage = () => undefined;

const SortableProviderCard = memo(function SortableProviderCard({
  provider,
  isCurrent,
  appId,
  isInConfig,
  isConfigStatusLoading,
  isOmo,
  isOmoSlim,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onTest,
  isTesting,
  isActionPending,
  isProxyRunning,
  isProxyTakeover,
  isAutoFailoverEnabled,
  failoverPriority,
  isInFailoverQueue,
  health,
  onToggleFailover,
  activeProviderId,
  isDefaultModel,
  onSetAsDefault,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  const dragHandleProps = useMemo(
    () => ({ attributes, listeners, isDragging }),
    [attributes, listeners, isDragging],
  );
  const handleToggleFailover = useCallback(
    (enabled: boolean) => onToggleFailover(provider.id, enabled),
    [onToggleFailover, provider.id],
  );
  const handleSetAsDefault = useMemo(
    () => (onSetAsDefault ? () => onSetAsDefault(provider) : undefined),
    [onSetAsDefault, provider],
  );

  return (
    <div ref={setNodeRef} style={style} data-provider-id={provider.id}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
        appId={appId}
        isInConfig={isInConfig}
        isConfigStatusLoading={isConfigStatusLoading}
        isOmo={isOmo}
        isOmoSlim={isOmoSlim}
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onRemoveFromConfig={onRemoveFromConfig}
        onDisableOmo={onDisableOmo}
        onDisableOmoSlim={onDisableOmoSlim}
        onDuplicate={onDuplicate}
        onConfigureUsage={onConfigureUsage ?? noopConfigureUsage}
        onOpenWebsite={onOpenWebsite}
        onOpenTerminal={onOpenTerminal}
        onTest={onTest}
        isTesting={isTesting}
        isActionPending={isActionPending}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        dragHandleProps={dragHandleProps}
        isAutoFailoverEnabled={isAutoFailoverEnabled}
        failoverPriority={failoverPriority}
        isInFailoverQueue={isInFailoverQueue}
        health={health}
        onToggleFailover={handleToggleFailover}
        activeProviderId={activeProviderId}
        // OpenClaw: default model
        isDefaultModel={isDefaultModel}
        onSetAsDefault={handleSetAsDefault}
      />
    </div>
  );
});
