import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { isUsageDashboardQueryKey } from "@/lib/query/usage";

/**
 * Coalescing window for `usage-log-recorded` events. The first event in a
 * window schedules one trailing invalidation; later events in the same window
 * are folded into it. Unlike a resetting debounce this cannot starve under a
 * steady event stream, and it caps refetches at one per window.
 */
export const USAGE_EVENT_COALESCE_MS = 2000;

/**
 * 监听后端 `usage-log-recorded` 事件，合并后 invalidate UsageDashboard 的
 * 汇总/趋势/统计/日志查询，让用户无需等待轮询周期。
 *
 * 后端在 `proxy_request_logs` 写入新行时会 emit 该事件（200ms 防抖合并），
 * 来源覆盖代理日志、Claude/Codex/Gemini 会话同步、启动归档。代理繁忙时事件
 * 约每秒一次，因此前端再按 USAGE_EVENT_COALESCE_MS 合并，避免每次都重跑
 * 全部聚合查询。只刷新依赖请求日志的 dashboard 查询，不触碰用量脚本、定价等。
 *
 * 该 hook 只挂在 UsageDashboard 上，避免在主界面其他位置无意义触发。
 */
export function useUsageEventBridge(
  coalesceMs: number = USAGE_EVENT_COALESCE_MS,
) {
  const queryClient = useQueryClient();
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Drop a pending flush on unmount.
  useEffect(
    () => () => {
      if (timerRef.current !== undefined) clearTimeout(timerRef.current);
      timerRef.current = undefined;
    },
    [],
  );

  useTauriEvent("usage-log-recorded", () => {
    if (timerRef.current !== undefined) return;
    timerRef.current = setTimeout(() => {
      timerRef.current = undefined;
      void queryClient.invalidateQueries(
        { predicate: (query) => isUsageDashboardQueryKey(query.queryKey) },
        // Don't cancel in-flight fetches: the Rust command keeps running anyway,
        // cancelling would only waste its result.
        { cancelRefetch: false },
      );
    }, coalesceMs);
  });
}
