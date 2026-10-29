/**
 * 供应商变更后的级联缓存失效（单一来源）
 *
 * 新增 / 更新 / 删除 / 切换 / 导入供应商后，除供应商列表外还会牵动多个
 * 派生缓存：OpenCode live ids 与 OMO / OMO Slim、OpenClaw live ids / 默认模型 /
 * 健康检查、Hermes 缓存、Claude Desktop 状态，以及对应供应商的用量脚本查询。
 * 以前这些失效逻辑在 mutations.ts 等处各写一遍，容易漏掉某一项；统一走这里。
 */

import type { QueryClient, QueryKey } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import { providerKeys } from "@/entities/provider/keys";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { invalidateHermesProviderCaches } from "@/hooks/useHermes";
import { omoKeys, omoSlimKeys } from "@/lib/query/omo";
import { usageKeys } from "@/lib/query/usage";

export interface InvalidateProviderGraphOptions {
  /** 需要同时失效用量脚本查询的供应商 ID（例如更新时的新旧 ID） */
  providerIds?: readonly string[];
}

function invalidateKeys(
  queryClient: QueryClient,
  keys: readonly QueryKey[],
): Promise<unknown> {
  return Promise.all(
    keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })),
  );
}

/** 各应用在供应商列表之外依赖供应商状态的缓存 */
const DEPENDENT_CACHES: Partial<
  Record<AppId, (queryClient: QueryClient) => Promise<unknown>>
> = {
  opencode: (queryClient) =>
    invalidateKeys(queryClient, [
      providerKeys.opencodeLiveIds,
      omoKeys.all,
      omoSlimKeys.all,
    ]),
  openclaw: (queryClient) =>
    invalidateKeys(queryClient, [
      openclawKeys.liveProviderIds,
      openclawKeys.defaultModel,
      openclawKeys.health,
    ]),
  hermes: (queryClient) => invalidateHermesProviderCaches(queryClient),
  "claude-desktop": (queryClient) =>
    invalidateKeys(queryClient, [
      providerKeys.claudeDesktopStatus,
      ["proxyStatus"],
    ]),
};

export async function invalidateProviderGraph(
  queryClient: QueryClient,
  appId: AppId,
  options: InvalidateProviderGraphOptions = {},
): Promise<void> {
  const providerIds = new Set(options.providerIds ?? []);

  await Promise.all([
    invalidateKeys(queryClient, [
      providerKeys.list(appId),
      ...[...providerIds].map((id) => usageKeys.script(id, appId)),
    ]),
    DEPENDENT_CACHES[appId]?.(queryClient),
  ]);
}
