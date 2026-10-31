import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach, afterAll } from "vitest";
import type { Provider } from "@/types";
import { useDragSort } from "@/hooks/useDragSort";
import { makeProvider, providerMap } from "../factories";

const updateSortOrderMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    updateSortOrder: (...args: unknown[]) => updateSortOrderMock(...args),
  },
}));

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = new QueryClient();

  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, queryClient };
}

const mockProviders: Record<string, Provider> = providerMap([
  makeProvider({ id: "a", name: "AAA", sortIndex: 1, createdAt: 5 }),
  makeProvider({ id: "b", name: "BBB", sortIndex: 0, createdAt: 10 }),
  makeProvider({ id: "c", name: "CCC", sortIndex: undefined, createdAt: 1 }),
]);

describe("useDragSort", () => {
  beforeEach(() => {
    updateSortOrderMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    consoleErrorSpy.mockClear();
  });

  afterAll(() => {
    consoleErrorSpy.mockRestore();
  });

  it("should sort providers by sortIndex, createdAt, and name", () => {
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    expect(result.current.sortedProviders.map((item) => item.id)).toEqual([
      "b",
      "a",
      "c",
    ]);
  });

  it("should call API and invalidate query cache after successful drag", async () => {
    updateSortOrderMock.mockResolvedValue(true);
    const { wrapper, queryClient } = createWrapper();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: { id: "a" },
      } as any);
    });

    expect(updateSortOrderMock).toHaveBeenCalledTimes(1);
    expect(updateSortOrderMock).toHaveBeenCalledWith(
      [
        { id: "a", sortIndex: 0 },
        { id: "b", sortIndex: 1 },
        { id: "c", sortIndex: 2 },
      ],
      "claude",
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
    });
    // A drop is its own feedback; no success toast (FE-PROV-13).
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("reorders the cached list before the backend answers and rolls back on error (FE-PROV-13)", async () => {
    let reject: (error: Error) => void = () => {};
    updateSortOrderMock.mockImplementation(
      () =>
        new Promise((_, r) => {
          reject = r;
        }),
    );
    const { wrapper, queryClient } = createWrapper();
    const cached = { providers: mockProviders, currentProviderId: "a" };
    queryClient.setQueryData(["providers", "claude"], cached);

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    let pending: Promise<void> = Promise.resolve();
    await act(async () => {
      pending = result.current.handleDragEnd({
        active: { id: "b" },
        over: { id: "a" },
      } as any);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    const optimistic = queryClient.getQueryData<typeof cached>([
      "providers",
      "claude",
    ]);
    expect(optimistic?.providers.a.sortIndex).toBe(0);
    expect(optimistic?.providers.b.sortIndex).toBe(1);

    await act(async () => {
      reject(new Error("network"));
      await pending;
    });
    expect(queryClient.getQueryData(["providers", "claude"])).toEqual(cached);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });

  it("should show error toast when drag operation fails", async () => {
    updateSortOrderMock.mockRejectedValue(new Error("network"));
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: { id: "a" },
      } as any);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(consoleErrorSpy).toHaveBeenCalled();
  });

  it("should not trigger API call when there is no valid target", async () => {
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: null,
      } as any);
    });

    expect(updateSortOrderMock).not.toHaveBeenCalled();
  });
});
