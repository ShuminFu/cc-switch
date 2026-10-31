/**
 * 累加模式应用读取 live 配置中已有供应商 ID 的查询（单一来源）
 *
 * 供应商列表用它判断「是否已加入配置」，复制供应商时用它避开已占用的 key。
 */

import type { QueryKey } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import { providersApi } from "@/lib/api/providers";
import { providerKeys } from "@/entities/provider/keys";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";

export interface LiveProviderIdsQuery {
  queryKey: QueryKey;
  queryFn: () => Promise<string[]>;
}

const LIVE_PROVIDER_IDS_QUERIES: Partial<Record<AppId, LiveProviderIdsQuery>> =
  {
    opencode: {
      queryKey: providerKeys.opencodeLiveIds,
      queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    },
    openclaw: {
      queryKey: openclawKeys.liveProviderIds,
      queryFn: () => providersApi.getOpenClawLiveProviderIds(),
    },
    hermes: {
      queryKey: hermesKeys.liveProviderIds,
      queryFn: () => providersApi.getHermesLiveProviderIds(),
    },
  };

/** 应用的 live 供应商 ID 查询；非累加模式应用返回 undefined */
export function liveProviderIdsQuery(
  appId: AppId,
): LiveProviderIdsQuery | undefined {
  return LIVE_PROVIDER_IDS_QUERIES[appId];
}
