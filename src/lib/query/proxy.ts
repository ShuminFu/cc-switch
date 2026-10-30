import { useSyncExternalStore } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type {
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyStatus,
} from "@/types/proxy";

// ========== 代理服务器状态 Hooks ==========
//
// The single proxy-status poller lives in `@/hooks/useProxyStatus` (key
// ["proxyStatus"], polled only while the server runs). The former duplicate
// 5 s `useProxyStatus` and the unused 2 s `useIsProxyRunning` /
// `useIsLiveTakeoverActive` pollers were removed (PERF-13).

/** Fallback refresh for takeover state changed outside this window. */
const TAKEOVER_FALLBACK_REFETCH_MS = 30_000;

/**
 * Whether the cached ["proxyStatus"] says the proxy server is running.
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
      queryClient.getQueryData<ProxyStatus>(["proxyStatus"])?.running === true,
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
    void queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
  });

  return useQuery({
    queryKey: ["proxyTakeoverStatus"],
    queryFn: () => proxyApi.getProxyTakeoverStatus(),
    refetchInterval: TAKEOVER_FALLBACK_REFETCH_MS,
    refetchIntervalInBackground: false,
  });
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
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
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
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
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
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
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
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
    },
    onError: (error: Error) => {
      toast.error(t("proxy.switchFailed", { error: error.message }));
    },
  });
}

// ========== Legacy 代理配置 Hooks (兼容) ==========

/**
 * 获取代理配置（旧版）
 */
export function useProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const { data: config, isLoading } = useQuery({
    queryKey: ["proxyConfig"],
    queryFn: () => proxyApi.getProxyConfig(),
  });

  const updateMutation = useMutation({
    mutationFn: proxyApi.updateProxyConfig,
    onSuccess: () => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });

  return {
    config,
    isLoading,
    updateConfig: updateMutation.mutateAsync,
    isUpdating: updateMutation.isPending,
  };
}

// ========== v3+ 全局/应用级配置 Hooks ==========

/**
 * 获取全局代理配置
 */
export function useGlobalProxyConfig() {
  return useQuery({
    queryKey: ["globalProxyConfig"],
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
      queryClient.invalidateQueries({ queryKey: ["globalProxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });
}

/**
 * 获取指定应用的代理配置
 */
export function useAppProxyConfig(appType: string) {
  return useQuery({
    queryKey: ["appProxyConfig", appType],
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
        queryKey: ["appProxyConfig", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["autoFailoverEnabled", variables.appType],
      });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["circuitBreakerConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });
}
