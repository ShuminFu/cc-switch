import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppId } from "@/lib/api";
import { useProviderActions } from "@/hooks/useProviderActions";
import { providerKeys } from "@/entities/provider/keys";
import type { Provider } from "@/types";

// 回归测试：mutation 进行中切换标签（activeApp 变化）后，onSuccess 仍须失效
// 发起时应用的缓存，而不是新激活应用的缓存。

const apiMocks = vi.hoisted(() => ({
  add: vi.fn(),
  update: vi.fn(),
  delete: vi.fn(),
  switch: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    add: (...args: unknown[]) => apiMocks.add(...args),
    update: (...args: unknown[]) => apiMocks.update(...args),
    delete: (...args: unknown[]) => apiMocks.delete(...args),
    switch: (...args: unknown[]) => apiMocks.switch(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {
    get: vi.fn().mockResolvedValue(null),
    applyClaudePluginConfig: vi.fn(),
  },
  openclawApi: {},
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    ({ app }: { app: AppId }) => useProviderActions(app),
    { wrapper, initialProps: { app: "opencode" as AppId } },
  );
  return { ...hook, invalidateSpy };
}

const provider: Provider = {
  id: "oc-provider",
  name: "OpenCode Provider",
  settingsConfig: {},
};

type Action = (actions: ReturnType<typeof useProviderActions>) => Promise<void>;

const cases: Array<[string, keyof typeof apiMocks, Action]> = [
  [
    "add",
    "add",
    (actions) =>
      actions.addProvider({
        name: provider.name,
        settingsConfig: {},
        providerKey: provider.id,
      }),
  ],
  ["update", "update", (actions) => actions.updateProvider(provider)],
  ["delete", "delete", (actions) => actions.deleteProvider(provider.id)],
  ["switch", "switch", (actions) => actions.switchProvider(provider)],
];

beforeEach(() => {
  for (const mock of Object.values(apiMocks)) {
    mock.mockReset().mockResolvedValue(true);
  }
});

describe("provider mutations bind to the variables' appId", () => {
  it.each(cases)(
    "%s invalidates the original app's caches after a tab change",
    async (_name, apiMethod, run) => {
      const pending = deferred();
      apiMocks[apiMethod].mockReturnValueOnce(pending.promise);
      const { result, rerender, invalidateSpy } = setup();

      let settled!: Promise<void>;
      await act(async () => {
        settled = run(result.current);
        await vi.waitFor(() => expect(apiMocks[apiMethod]).toHaveBeenCalled());
      });
      expect(apiMocks[apiMethod].mock.calls[0]).toContain("opencode");

      rerender({ app: "claude" });

      await act(async () => {
        pending.resolve(true);
        await settled;
      });

      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: providerKeys.list("opencode"),
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: providerKeys.opencodeLiveIds,
      });
      expect(invalidateSpy).not.toHaveBeenCalledWith({
        queryKey: providerKeys.list("claude"),
      });
    },
  );
});
