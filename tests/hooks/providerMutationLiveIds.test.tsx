import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppId } from "@/lib/api";
import {
  useAddProviderMutation,
  useDeleteProviderMutation,
  useUpdateProviderMutation,
} from "@/lib/query/mutations";
import { providerKeys } from "@/entities/provider/keys";
import { openclawKeys } from "@/hooks/useOpenClaw";
import type { Provider } from "@/types";

// 回归测试：OpenCode / OpenClaw 新增（默认 addToLive）、更新、删除供应商后，
// 已挂载的列表必须立即刷新 live ids，否则卡片会显示“未在配置中”直到窗口重新聚焦。

const apiMocks = vi.hoisted(() => ({
  add: vi.fn(),
  update: vi.fn(),
  delete: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    add: (...args: unknown[]) => apiMocks.add(...args),
    update: (...args: unknown[]) => apiMocks.update(...args),
    delete: (...args: unknown[]) => apiMocks.delete(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {},
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const provider: Provider = {
  id: "live-provider",
  name: "Live Provider",
  settingsConfig: {},
};

const LIVE_ID_KEYS: Record<"opencode" | "openclaw", readonly unknown[]> = {
  opencode: providerKeys.opencodeLiveIds,
  openclaw: openclawKeys.liveProviderIds,
};

type Run = (appId: AppId, wrapper: Wrapper) => Promise<unknown>;
type Wrapper = ({ children }: { children: ReactNode }) => JSX.Element;

const runAdd: Run = async (appId, wrapper) => {
  const { result } = renderHook(() => useAddProviderMutation(), { wrapper });
  return act(() =>
    result.current.mutateAsync({
      appId,
      provider: {
        name: provider.name,
        settingsConfig: {},
        providerKey: provider.id,
      },
    }),
  );
};

const runUpdate: Run = async (appId, wrapper) => {
  const { result } = renderHook(() => useUpdateProviderMutation(), {
    wrapper,
  });
  return act(() => result.current.mutateAsync({ appId, provider }));
};

const runDelete: Run = async (appId, wrapper) => {
  const { result } = renderHook(() => useDeleteProviderMutation(), {
    wrapper,
  });
  return act(() =>
    result.current.mutateAsync({ appId, providerId: provider.id }),
  );
};

const mutations: Array<[string, Run]> = [
  ["add", runAdd],
  ["update", runUpdate],
  ["delete", runDelete],
];

const cases = (["opencode", "openclaw"] as const).flatMap((appId) =>
  mutations.map(([name, run]) => [name, appId, run] as const),
);

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper: Wrapper = ({ children }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper, invalidateSpy };
}

beforeEach(() => {
  for (const mock of Object.values(apiMocks)) {
    mock.mockReset().mockResolvedValue(true);
  }
});

describe("provider mutations refresh live provider ids", () => {
  it.each(cases)(
    "%s on %s invalidates the live-id key",
    async (_name, appId, run) => {
      const { wrapper, invalidateSpy } = setup();

      await run(appId, wrapper);

      const keys = invalidateSpy.mock.calls.map(
        ([filters]) => filters?.queryKey,
      );
      expect(keys).toContainEqual(LIVE_ID_KEYS[appId]);
      expect(keys).toContainEqual(providerKeys.list(appId));
    },
  );
});
