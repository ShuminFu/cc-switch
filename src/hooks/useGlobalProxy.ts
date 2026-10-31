/**
 * 全局出站代理 React Hooks
 *
 * 提供获取、设置和测试全局代理的 React Query hooks。
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getGlobalProxyUrl,
  setGlobalProxyUrl,
  testProxyUrl,
  scanLocalProxies,
  type ProxyTestResult,
  type DetectedProxy,
} from "@/lib/api/globalProxy";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 获取全局代理 URL
 */
export function useGlobalProxyUrl() {
  return useQuery({
    queryKey: queryKeys.proxy.globalUrl,
    queryFn: getGlobalProxyUrl,
    staleTime: 30 * 1000, // 30秒内不重新获取，避免展开时闪烁
  });
}

/**
 * 设置全局代理 URL
 */
export function useSetGlobalProxyUrl() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: setGlobalProxyUrl,
    onSuccess: () => {
      toast.success(t("settings.globalProxy.saved"));
      queryClient.invalidateQueries({ queryKey: queryKeys.proxy.globalUrl });
      queryClient.invalidateQueries({
        queryKey: queryKeys.proxy.upstreamStatus,
      });
    },
    onError: (error: unknown) => {
      const message = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("settings.globalProxy.saveFailed", { error: message }));
    },
  });
}

/**
 * 测试代理连接
 */
export function useTestProxy() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: testProxyUrl,
    onSuccess: (result: ProxyTestResult) => {
      if (result.success) {
        toast.success(
          t("settings.globalProxy.testSuccess", { latency: result.latencyMs }),
        );
      } else {
        toast.error(
          t("settings.globalProxy.testFailed", { error: result.error }),
        );
      }
    },
    onError: (error: unknown) => {
      toast.error(
        t("settings.globalProxy.testFailed", {
          error: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    },
  });
}

/**
 * 扫描本地代理
 */
export function useScanProxies() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: scanLocalProxies,
    onError: (error: unknown) => {
      toast.error(
        t("settings.globalProxy.scanFailed", {
          error: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    },
  });
}

export type { DetectedProxy };
