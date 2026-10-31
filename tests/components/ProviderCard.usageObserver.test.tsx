import { render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { usageKeys } from "@/lib/query/usage";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";

const usageQueryMock = vi.fn();

vi.mock("@/lib/api/usage", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/usage")>();
  return {
    ...actual,
    usageApi: {
      ...actual.usageApi,
      query: (...args: unknown[]) => usageQueryMock(...args),
    },
  };
});

const provider: Provider = {
  id: "p1",
  name: "Relay",
  category: "custom",
  settingsConfig: { baseUrl: "https://relay.example.com/v1" },
  meta: {
    usage_script: {
      enabled: true,
      language: "javascript",
      code: "",
      autoQueryInterval: 5,
    },
  },
};

function renderCard(appId: AppId) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <ProviderCard
        provider={provider}
        appId={appId}
        isCurrent={false}
        isInConfig
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
        isProxyRunning={false}
      />
    </QueryClientProvider>,
  );
  return client;
}

// PERF-47: the card and its UsageFooter share one usage-script observer.
describe("ProviderCard usage query", () => {
  it("registers a single observer that auto-refreshes in-config OpenClaw providers", async () => {
    usageQueryMock.mockResolvedValue({
      success: true,
      data: [{ planName: "basic", remaining: 1, unit: "USD" }],
    });
    const client = renderCard("openclaw");
    await waitFor(() => expect(usageQueryMock).toHaveBeenCalled());

    const query = client
      .getQueryCache()
      .find({ queryKey: usageKeys.script(provider.id, "openclaw") });
    expect(query?.getObserversCount()).toBe(1);
    const observer = query?.observers[0];
    expect(observer?.options.refetchInterval).toBe(5 * 60 * 1000);
    expect(observer?.options.refetchIntervalInBackground).toBe(false);
  });
});
