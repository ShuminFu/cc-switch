import type { ReactNode } from "react";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useRequestLogs } from "@/lib/query/usage";
import { createTestQueryClient } from "../utils/testQueryClient";

const getRequestLogsMock = vi.fn();
vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getRequestLogs: (...args: unknown[]) => getRequestLogsMock(...args),
  },
}));

// FE-ARCH-41: later pages use a frozen window instead of `now`.
describe("useRequestLogs paging window", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date("2026-10-01T12:00:00Z"));
    getRequestLogsMock.mockReset().mockResolvedValue({ data: [], total: 25 });
  });
  afterEach(() => vi.useRealTimers());

  it("freezes endDate when leaving page 0 and keeps it across refetches", async () => {
    const queryClient = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const filters = {};
    const range = { preset: "today" as const };
    const { rerender, result } = renderHook(
      ({ page }) =>
        useRequestLogs({
          filters,
          range,
          page,
          options: { refetchInterval: false },
        }),
      { wrapper, initialProps: { page: 0 } },
    );
    await waitFor(() => expect(getRequestLogsMock).toHaveBeenCalledTimes(1));
    const firstEnd = getRequestLogsMock.mock.calls[0][0].endDate;

    vi.setSystemTime(new Date("2026-10-01T12:05:00Z"));
    rerender({ page: 1 });
    await waitFor(() => expect(getRequestLogsMock).toHaveBeenCalledTimes(2));
    const frozenEnd = getRequestLogsMock.mock.calls[1][0].endDate;
    expect(frozenEnd).toBeGreaterThan(firstEnd);

    vi.setSystemTime(new Date("2026-10-01T12:10:00Z"));
    await act(async () => {
      await result.current.refetch();
    });
    const refetchEnd = getRequestLogsMock.mock.calls.at(-1)?.[0].endDate;
    expect(refetchEnd).toBe(frozenEnd);

    rerender({ page: 2 });
    await waitFor(() => expect(getRequestLogsMock).toHaveBeenCalledTimes(4));
    expect(getRequestLogsMock.mock.calls[3][0].endDate).toBe(frozenEnd);
  });
});
