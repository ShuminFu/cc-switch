import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { useDuplicateProvider } from "@/hooks/useDuplicateProvider";
import { createTestQueryClient } from "../utils/testQueryClient";

const getAllMock = vi.fn();
const updateSortOrderMock = vi.fn();

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      getAll: (...args: unknown[]) => getAllMock(...args),
      updateSortOrder: (...args: unknown[]) => updateSortOrderMock(...args),
    },
  };
});

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

const provider = (id: string, sortIndex: number, extra = {}): Provider => ({
  id,
  name: id,
  settingsConfig: { env: {} },
  sortIndex,
  ...extra,
});

function setup(
  providers: Record<string, Provider>,
  addProvider: (p: Omit<Provider, "id">) => Promise<Provider | void>,
) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(
    () => useDuplicateProvider("claude", providers, addProvider),
    { wrapper },
  );
}

// FE-ARCH-43
describe("useDuplicateProvider", () => {
  let store: Record<string, Provider>;

  beforeEach(() => {
    store = {
      a: provider("a", 0, {
        meta: {
          promptCacheKey: "pck",
          partnerPromotionKey: "promo",
          isPartner: true,
          apiFormat: "anthropic",
        },
      }),
      b: provider("b", 1),
      c: provider("c", 2),
    };
    getAllMock.mockReset().mockImplementation(async () => ({ ...store }));
    updateSortOrderMock
      .mockReset()
      .mockImplementation(
        async (updates: { id: string; sortIndex: number }[]) => {
          for (const u of updates)
            store[u.id] = { ...store[u.id], sortIndex: u.sortIndex };
        },
      );
  });

  it("does not touch the sort order when the add fails", async () => {
    const addProvider = vi.fn().mockRejectedValue(new Error("invalid"));
    const { result } = setup(store, addProvider);
    await act(async () => {
      await result.current(store.a);
    });
    expect(addProvider).toHaveBeenCalledTimes(1);
    expect(updateSortOrderMock).not.toHaveBeenCalled();
  });

  it("drops cache-routing and partner meta from the copy", async () => {
    const addProvider = vi.fn(async (p: Omit<Provider, "id">) => {
      store.copy = { ...p, id: "copy" } as Provider;
      return store.copy;
    });
    const { result } = setup(store, addProvider);
    await act(async () => {
      await result.current(store.a);
    });
    const payload = addProvider.mock.calls[0][0];
    expect(payload.meta).toEqual({ apiFormat: "anthropic" });
    expect(payload.sortIndex).toBe(1);
  });

  it("two quick duplicates end with distinct, contiguous sort indices", async () => {
    let n = 0;
    const addProvider = vi.fn(async (p: Omit<Provider, "id">) => {
      n += 1;
      const id = `copy${n}`;
      store[id] = { ...p, id } as Provider;
      return store[id];
    });
    const { result } = setup(store, addProvider);
    await act(async () => {
      await Promise.all([result.current(store.a), result.current(store.a)]);
    });
    const indices = Object.values(store)
      .map((p) => p.sortIndex)
      .sort((x, y) => (x ?? 0) - (y ?? 0));
    expect(indices).toEqual([0, 1, 2, 3, 4]);
    expect(store.a.sortIndex).toBe(0);
    expect(store.b.sortIndex).toBe(3);
    expect(store.c.sortIndex).toBe(4);
  });
});
