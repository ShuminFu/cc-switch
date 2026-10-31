import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useProviderHealth } from "@/lib/query/failover";
import {
  fetchProxyOverview,
  useCachedProxyRunning,
  useProxyTakeoverStatus,
} from "@/lib/query/proxy";
import { emitTauriEvent } from "../msw/tauriMocks";
import { createTestQueryClient } from "../utils/testQueryClient";

const getProviderHealthMock = vi.fn();
const getProxyOverviewMock = vi.fn();

vi.mock("@/lib/api/failover", () => ({
  failoverApi: {
    getProviderHealth: (...args: unknown[]) => getProviderHealthMock(...args),
  },
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getProxyOverview: (...args: unknown[]) => getProxyOverviewMock(...args),
  },
}));

function setup<T>(hook: () => T) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, ...renderHook(hook, { wrapper }) };
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("proxy-related polling (PERF-13)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    getProviderHealthMock.mockReset().mockResolvedValue({ is_healthy: true });
    getProxyOverviewMock
      .mockReset()
      .mockResolvedValue({ status: { running: false }, takeover: {} });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("useCachedProxyRunning follows the cached proxyStatus without fetching", async () => {
    const { queryClient, result } = setup(() => useCachedProxyRunning());
    expect(result.current).toBe(false);

    act(() => {
      queryClient.setQueryData(["proxyStatus"], { running: true });
    });
    expect(result.current).toBe(true);

    act(() => {
      queryClient.setQueryData(["proxyStatus"], { running: false });
    });
    expect(result.current).toBe(false);
  });

  it("provider health polls only while the proxy is running", async () => {
    const { queryClient, unmount } = setup(() =>
      useProviderHealth("p1", "claude"),
    );
    await advance(0);
    expect(getProviderHealthMock).toHaveBeenCalledTimes(1);

    // Proxy stopped: no polling.
    await advance(20_000);
    expect(getProviderHealthMock).toHaveBeenCalledTimes(1);

    // Proxy running: polls every 5 s.
    act(() => {
      queryClient.setQueryData(["proxyStatus"], { running: true });
    });
    await advance(10_000);
    expect(getProviderHealthMock.mock.calls.length).toBeGreaterThanOrEqual(3);

    // Stopped again: polling stops.
    act(() => {
      queryClient.setQueryData(["proxyStatus"], { running: false });
    });
    const calls = getProviderHealthMock.mock.calls.length;
    await advance(20_000);
    expect(getProviderHealthMock).toHaveBeenCalledTimes(calls);
    unmount();
  });

  it("takeover status refreshes on proxy-flags-changed instead of a 2 s poll", async () => {
    const { unmount } = setup(() => useProxyTakeoverStatus());
    await advance(0);
    expect(getProxyOverviewMock).toHaveBeenCalledTimes(1);

    await advance(10_000);
    expect(getProxyOverviewMock).toHaveBeenCalledTimes(1);

    emitTauriEvent("proxy-flags-changed", {});
    await advance(0);
    expect(getProxyOverviewMock).toHaveBeenCalledTimes(2);
    unmount();
  });

  it("status and takeover share one get_proxy_overview call and both caches", async () => {
    getProxyOverviewMock.mockResolvedValue({
      status: { running: true },
      takeover: { claude: true },
    });
    const queryClient = createTestQueryClient();
    const [a, b] = await Promise.all([
      fetchProxyOverview(queryClient),
      fetchProxyOverview(queryClient),
    ]);
    expect(a).toBe(b);
    expect(getProxyOverviewMock).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData(["proxyStatus"])).toEqual({
      running: true,
    });
    expect(queryClient.getQueryData(["proxyTakeoverStatus"])).toEqual({
      claude: true,
    });

    // The in-flight slot is released once the call settles.
    await fetchProxyOverview(queryClient);
    expect(getProxyOverviewMock).toHaveBeenCalledTimes(2);
  });
});
