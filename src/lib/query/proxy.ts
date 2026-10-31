import { useSyncExternalStore } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useTauriEvent } from "@/lib/useTauriEvent";
import type {
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyStatus,
} from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getAppLabel, isAnyTakeoverActive } from "@/config/proxyApps";
import { queryKeys } from "./keys";

// ========== 代理服务器状态 Hooks ==========
//
// This module is the single proxy query layer (FE-ARCH-05). `useProxyStatus`
// below is the only `get_proxy_status` poller (key `queryKeys.proxy.status`,
// polled only while the server runs); `@/hooks/useProxyStatus` re-exports it.

/** Fallback refresh for takeover state changed outside this window. */
const TAKEOVER_FALLBACK_REFETCH_MS = 30_000;

/**
 * Whether the cached queryKeys.proxy.status says the proxy server is running.
 *
 * Reads the cache only (never fetches), so components can gate their own
 * polling on proxy state without adding another `get_proxy_status` poller.
 */
export function useCachedProxyRunning(): boolean {
  const queryClient = useQueryClient();
  const queryCache = queryClient.getQueryCache();
  return useSyncExternalStore(
    (onChange) => queryCache.subscribe(onChange),
    () =>
      queryClient.getQueryData<ProxyStatus>(queryKeys.proxy.status)?.running ===
      true,
  );
}

/**
 * 获取各应用接管状态
 *
 * Takeover changes made from this window go through mutations that invalidate
 * this key; changes made from the tray emit `proxy-flags-changed`. A slow
 * fallback poll covers anything else, and pauses while the window is hidden.
 */
export function useProxyTakeoverStatus() {
  const queryClient = useQueryClient();

  useTauriEvent("proxy-flags-changed", () => {
    void Promise.all([
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      }),
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status }),
    ]);
  });

  return useQuery({
    queryKey: queryKeys.proxy.takeoverStatus,
    queryFn: () => proxyApi.getProxyTakeoverStatus(),
    // Opt out of the global staleTime: the tray can change this at any time.
    staleTime: 0,
    refetchInterval: TAKEOVER_FALLBACK_REFETCH_MS,
    refetchIntervalInBackground: false,
    placeholderData: (previousData) => previousData,
  });
}

/**
 * 代理服务状态管理
 */
export function useProxyStatus() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // 查询状态（自动轮询）
  const { data: status, isLoading } = useQuery({
    queryKey: queryKeys.proxy.status,
    queryFn: () => proxyApi.getProxyStatus(),
    // Always refresh on focus/mount (global staleTime does not apply).
    staleTime: 0,
    // 仅在服务运行时轮询
    refetchInterval: (query) => (query.state.data?.running ? 2000 : false),
    // 保持之前的数据，避免闪烁
    placeholderData: (previousData) => previousData,
  });

  // 查询各应用接管状态（与 useProxyTakeoverStatus 共用同一查询与事件订阅）
  const { data: takeoverStatus } = useProxyTakeoverStatus();

  // 启动服务器（总开关：仅启动服务，不接管）
  const startProxyServerMutation = useMutation({
    mutationFn: () => proxyApi.startProxyServer(),
    onSuccess: (info) => {
      toast.success(
        t("proxy.server.started", {
          address: info.address,
          port: info.port,
          defaultValue: `代理服务已启动 - ${info.address}:${info.port}`,
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
    },
    onError: (error: unknown) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.server.startFailed", {
          detail,
          defaultValue: `启动代理服务失败: ${detail}`,
        }),
      );
    },
  });

  // 停止服务器（仅停止服务，不改写/恢复其它应用接管状态）
  const stopProxyServerMutation = useMutation({
    mutationFn: () => proxyApi.stopProxyServer(),
    onSuccess: () => {
      toast.success(
        t("proxy.server.stopped", {
          defaultValue: "Routing service stopped",
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
    },
    onError: (error: unknown) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.server.stopFailed", {
          detail,
          defaultValue: "Failed to stop routing service: {{detail}}",
        }),
      );
    },
  });

  // 停止服务器（总开关关闭：强制恢复所有已接管的 Live 配置）
  const stopWithRestoreMutation = useMutation({
    mutationFn: () => proxyApi.stopProxyWithRestore(),
    onSuccess: () => {
      toast.success(
        t("proxy.stoppedWithRestore", {
          defaultValue: "代理服务已关闭，已恢复所有接管配置",
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      });
      // 彻底删除所有供应商健康状态缓存（后端已清空数据库记录）
      queryClient.removeQueries({
        queryKey: queryKeys.failover.providerHealthAll,
      });
      // 彻底删除所有熔断器统计缓存（代理停止后熔断器状态已重置）
      queryClient.removeQueries({
        queryKey: queryKeys.failover.circuitBreakerStatsAll,
      });
      // 注意：故障转移队列和开关状态会保留，不需要刷新
    },
    onError: (error: unknown) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.stopWithRestoreFailed", {
          detail,
          defaultValue: `停止失败: ${detail}`,
        }),
      );
    },
  });

  // 按应用开启/关闭接管
  const setTakeoverForAppMutation = useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      proxyApi.setProxyTakeoverForApp(appType, enabled),
    onSuccess: (_data, variables) => {
      const appLabel = getAppLabel(variables.appType);

      toast.success(
        variables.enabled
          ? t("proxy.takeover.enabled", {
              app: appLabel,
              defaultValue: `已接管 ${appLabel} 配置（请求将走本地代理）`,
            })
          : t("proxy.takeover.disabled", {
              app: appLabel,
              defaultValue: `已恢复 ${appLabel} 配置`,
            }),
        { closeButton: true },
      );

      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      });
    },
    onError: (error: unknown) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.takeover.failed", {
          detail,
          defaultValue: `操作失败: ${detail}`,
        }),
      );
    },
  });

  // 代理模式切换供应商（热切换）
  const switchProxyProviderMutation = useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => proxyApi.switchProxyProvider(appType, providerId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
    },
    onError: (error: unknown) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.switchFailed", {
          error: detail,
          defaultValue: `切换失败: ${detail}`,
        }),
      );
    },
  });

  // 检查是否运行中
  const checkRunning = async () => {
    try {
      return await proxyApi.isProxyRunning();
    } catch {
      return false;
    }
  };

  // 检查接管状态
  const checkTakeoverActive = async () => {
    try {
      return await proxyApi.isLiveTakeoverActive();
    } catch {
      return false;
    }
  };

  return {
    status,
    isLoading,
    isRunning: status?.running || false,
    takeoverStatus,
    isTakeoverActive: isAnyTakeoverActive(takeoverStatus),

    // 启动/停止（总开关）
    startProxyServer: startProxyServerMutation.mutateAsync,
    stopProxyServer: stopProxyServerMutation.mutateAsync,
    stopWithRestore: stopWithRestoreMutation.mutateAsync,

    // 按应用接管开关
    setTakeoverForApp: setTakeoverForAppMutation.mutateAsync,

    // 代理模式下切换供应商
    switchProxyProvider: switchProxyProviderMutation.mutateAsync,

    // 状态检查
    checkRunning,
    checkTakeoverActive,

    // 加载状态
    isStarting: startProxyServerMutation.isPending,
    isStoppingServer: stopProxyServerMutation.isPending,
    isStopping: stopWithRestoreMutation.isPending,
    isPending:
      startProxyServerMutation.isPending ||
      stopProxyServerMutation.isPending ||
      stopWithRestoreMutation.isPending ||
      setTakeoverForAppMutation.isPending,
  };
}

// ========== 代理服务器控制 Hooks ==========

/**
 * 启动代理服务器
 */
export function useStartProxyServer() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.startProxyServer(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.running });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.liveTakeoverActive,
      });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      });
    },
  });
}

/**
 * 停止代理服务器
 */
export function useStopProxyServer() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.stopProxyWithRestore(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.running });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.liveTakeoverActive,
      });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      });
    },
  });
}

/**
 * 设置应用接管状态
 */
export function useSetProxyTakeoverForApp() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      proxyApi.setProxyTakeoverForApp(appType, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.takeoverStatus,
      });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.liveTakeoverActive,
      });
    },
  });
}

/**
 * 代理模式下切换供应商
 */
export function useSwitchProxyProvider() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => proxyApi.switchProxyProvider(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
      queryClient.invalidateQueries({
        queryKey: queryKeys.providers.byApp(variables.appType),
      });
    },
    onError: (error: unknown) => {
      toast.error(
        t("proxy.switchFailed", { error: extractErrorMessage(error) }),
      );
    },
  });
}

// ========== v3+ 全局/应用级配置 Hooks ==========

/**
 * 获取全局代理配置
 */
export function useGlobalProxyConfig() {
  return useQuery({
    queryKey: queryKeys.proxy.globalConfig,
    queryFn: () => proxyApi.getGlobalProxyConfig(),
  });
}

/**
 * 更新全局代理配置
 */
export function useUpdateGlobalProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: GlobalProxyConfig) =>
      proxyApi.updateGlobalProxyConfig(config),
    onSuccess: () => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.globalConfig });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.config });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
    },
    onError: (error: unknown) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", {
          error: extractErrorMessage(error),
        }),
      );
    },
  });
}

/**
 * 获取指定应用的代理配置
 */
export function useAppProxyConfig(appType: string) {
  return useQuery({
    queryKey: queryKeys.proxy.appConfig(appType),
    queryFn: () => proxyApi.getProxyConfigForApp(appType),
    enabled: !!appType,
  });
}

/**
 * 更新指定应用的代理配置
 */
export function useUpdateAppProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: AppProxyConfig) =>
      proxyApi.updateProxyConfigForApp(config),
    onSuccess: (_, variables) => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.appConfig(variables.appType),
      });
      queryClient.invalidateQueries({
        queryKey: queryKeys.failover.autoEnabled(variables.appType),
      });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.config });
      queryClient.invalidateQueries({
        queryKey: queryKeys.failover.circuitBreakerConfig,
      });
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.status });
    },
    onError: (error: unknown) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", {
          error: extractErrorMessage(error),
        }),
      );
    },
  });
}
