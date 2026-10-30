import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  USAGE_EVENT_COALESCE_MS,
  useUsageEventBridge,
} from "@/hooks/useUsageEventBridge";
import { isUsageDashboardQueryKey, usageKeys } from "@/lib/query/usage";
import { emitTauriEvent } from "../msw/tauriMocks";
import { createTestQueryClient } from "../utils/testQueryClient";

function setup() {
  const queryClient = createTestQueryClient();
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useUsageEventBridge(), { wrapper });
  return { queryClient, invalidateSpy, ...hook };
}

/** Let the async `listen` registration inside the effect resolve. */
async function flushListen() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useUsageEventBridge", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces a burst of events into one trailing invalidation", async () => {
    const { invalidateSpy, unmount } = setup();
    await flushListen();

    for (let i = 0; i < 5; i++) {
      emitTauriEvent("usage-log-recorded", null);
      act(() => {
        vi.advanceTimersByTime(300);
      });
    }
    expect(invalidateSpy).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(USAGE_EVENT_COALESCE_MS);
    });
    expect(invalidateSpy).toHaveBeenCalledTimes(1);
    expect(invalidateSpy.mock.calls[0][1]).toEqual({ cancelRefetch: false });
    unmount();
  });

  it("does not starve under a steady event stream (max one per window)", async () => {
    const { invalidateSpy, unmount } = setup();
    await flushListen();

    // One event per second for 10 seconds.
    for (let i = 0; i < 10; i++) {
      emitTauriEvent("usage-log-recorded", null);
      act(() => {
        vi.advanceTimersByTime(1000);
      });
    }
    const calls = invalidateSpy.mock.calls.length;
    expect(calls).toBeGreaterThanOrEqual(4);
    expect(calls).toBeLessThanOrEqual(10_000 / USAGE_EVENT_COALESCE_MS);
    unmount();
  });

  it("only invalidates usage-dashboard query keys", async () => {
    const { queryClient, unmount } = setup();
    await flushListen();

    const dashboardKey = usageKeys.summary("today", undefined, undefined);
    const logsKey = usageKeys.logs({ preset: "today" }, 0, 20);
    const scriptKey = usageKeys.script("p1", "claude");
    const pricingKey = usageKeys.pricing();
    const otherKey = ["providers", "claude"];
    for (const key of [
      dashboardKey,
      logsKey,
      scriptKey,
      pricingKey,
      otherKey,
    ]) {
      queryClient.setQueryData(key, { v: 1 });
    }

    emitTauriEvent("usage-log-recorded", null);
    act(() => {
      vi.advanceTimersByTime(USAGE_EVENT_COALESCE_MS);
    });

    const invalidated = (key: readonly unknown[]) =>
      queryClient.getQueryState(key)?.isInvalidated;
    expect(invalidated(dashboardKey)).toBe(true);
    expect(invalidated(logsKey)).toBe(true);
    expect(invalidated(scriptKey)).toBe(false);
    expect(invalidated(pricingKey)).toBe(false);
    expect(invalidated(otherKey)).toBe(false);
    unmount();
  });

  it("drops a pending invalidation on unmount", async () => {
    const { invalidateSpy, unmount } = setup();
    await flushListen();

    emitTauriEvent("usage-log-recorded", null);
    unmount();
    act(() => {
      vi.advanceTimersByTime(USAGE_EVENT_COALESCE_MS * 2);
    });
    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});

describe("isUsageDashboardQueryKey", () => {
  it("matches aggregate and log keys but not script/pricing/limits/detail", () => {
    expect(
      isUsageDashboardQueryKey(usageKeys.trends("7d", undefined, undefined)),
    ).toBe(true);
    expect(
      isUsageDashboardQueryKey(
        usageKeys.modelStats("7d", undefined, undefined),
      ),
    ).toBe(true);
    expect(isUsageDashboardQueryKey(usageKeys.detail("r1"))).toBe(false);
    expect(isUsageDashboardQueryKey(usageKeys.limits("p", "claude"))).toBe(
      false,
    );
    expect(isUsageDashboardQueryKey(["other", "summary"])).toBe(false);
  });
});
