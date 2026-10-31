import type { ReactNode } from "react";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useRequestLogs } from "@/lib/query/usage";
import { mockInvoke, type RecordedInvoke } from "../msw/invokeRecorder";
import { createTestQueryClient } from "../utils/testQueryClient";

// TEST-12: mocked at the IPC boundary, so the real usageApi wrapper runs.
let calls: RecordedInvoke[] = [];
const endDateOf = (index: number) =>
  (calls[index].args.filters as { endDate: number }).endDate;

// FE-ARCH-41: later pages use a frozen window instead of `now`.
describe("useRequestLogs paging window", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date("2026-10-01T12:00:00Z"));
    calls = mockInvoke({
      get_request_logs: { data: [], total: 25, page: 0, pageSize: 20 },
    }).calls;
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
    await waitFor(() => expect(calls).toHaveLength(1));
    const firstEnd = endDateOf(0);

    vi.setSystemTime(new Date("2026-10-01T12:05:00Z"));
    rerender({ page: 1 });
    await waitFor(() => expect(calls).toHaveLength(2));
    const frozenEnd = endDateOf(1);
    expect(frozenEnd).toBeGreaterThan(firstEnd);

    vi.setSystemTime(new Date("2026-10-01T12:10:00Z"));
    await act(async () => {
      await result.current.refetch();
    });
    const refetchEnd = endDateOf(calls.length - 1);
    expect(refetchEnd).toBe(frozenEnd);

    rerender({ page: 2 });
    await waitFor(() => expect(calls).toHaveLength(4));
    expect(endDateOf(3)).toBe(frozenEnd);
    expect(calls[3].args).toMatchObject({ page: 2, pageSize: 20 });
  });
});
