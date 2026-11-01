import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useDeleteProviderMutation,
  useSwitchProviderMutation,
} from "@/lib/query/mutations";
import { queryKeys } from "@/lib/query/keys";
import type { Provider } from "@/types";

const api = vi.hoisted(() => ({
  switch: vi.fn(),
  delete: vi.fn(),
  add: vi.fn(),
  updateTrayMenu: vi.fn(async () => true),
}));
const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: api,
  sessionsApi: {},
  settingsApi: {},
}));
vi.mock("sonner", () => ({ toast: toastMocks }));

const a: Provider = {
  id: "a",
  name: "A",
  settingsConfig: { env: { X: "1" } },
  sortIndex: 0,
  meta: { apiFormat: "anthropic" },
};
const b: Provider = { id: "b", name: "B", settingsConfig: {}, sortIndex: 1 };

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  // Keep the seeded cache: no queryFn is registered for it in this test.
  vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue();
  queryClient.setQueryData(queryKeys.providers.byApp("claude"), {
    providers: { a, b },
    currentProviderId: "a",
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

const cache = (qc: QueryClient) =>
  qc.getQueryData<{
    providers: Record<string, Provider>;
    currentProviderId: string;
  }>(queryKeys.providers.byApp("claude"));

// FE-ARCH-18 / FE-ARCH-17
describe("optimistic provider mutations", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
    api.updateTrayMenu.mockResolvedValue(true);
    toastMocks.success.mockReset();
    toastMocks.error.mockReset();
  });

  it("switch highlights the target immediately and rolls back on failure", async () => {
    let reject!: (e: unknown) => void;
    api.switch.mockImplementation(() => new Promise((_, r) => (reject = r)));
    const { queryClient, wrapper } = setup();
    const { result } = renderHook(() => useSwitchProviderMutation("claude"), {
      wrapper,
    });

    act(() => {
      result.current.mutate("b");
    });
    await waitFor(() =>
      expect(cache(queryClient)?.currentProviderId).toBe("b"),
    );

    await act(async () => {
      reject("port in use");
    });
    await waitFor(() =>
      expect(cache(queryClient)?.currentProviderId).toBe("a"),
    );
    expect(toastMocks.error).toHaveBeenCalledTimes(1);
  });

  it("delete removes the card at once and restores it on failure", async () => {
    api.delete.mockRejectedValue("locked");
    const { queryClient, wrapper } = setup();
    const { result } = renderHook(() => useDeleteProviderMutation("claude"), {
      wrapper,
    });
    await act(async () => {
      await result.current.mutateAsync("b").catch(() => undefined);
    });
    expect(cache(queryClient)?.providers.b).toEqual(b);
    expect(toastMocks.error).toHaveBeenCalledTimes(1);
  });

  it("Undo after delete re-adds the identical provider", async () => {
    api.delete.mockResolvedValue(undefined);
    api.add.mockResolvedValue(true);
    const { queryClient, wrapper } = setup();
    const { result } = renderHook(() => useDeleteProviderMutation("claude"), {
      wrapper,
    });
    await act(async () => {
      await result.current.mutateAsync("a");
    });
    expect(cache(queryClient)?.providers.a).toBeUndefined();

    const [, options] = toastMocks.success.mock.calls.at(-1) ?? [];
    expect(options?.action?.label).toBeTruthy();
    await act(async () => {
      options.action.onClick();
      await Promise.resolve();
    });
    await waitFor(() => expect(api.add).toHaveBeenCalledTimes(1));
    expect(api.add).toHaveBeenCalledWith(a, "claude", false);
  });
});
