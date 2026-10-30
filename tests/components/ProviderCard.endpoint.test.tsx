import type { ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  ProviderCard,
  getProviderCardUrl,
} from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { mockInvoke } from "../msw/invokeRecorder";

function renderWithClient(node: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{node}</QueryClientProvider>,
  );
}

const makeProvider = (
  settingsConfig: Record<string, unknown>,
  extra: Partial<Provider> = {},
): Provider => ({
  id: "p1",
  name: "Relay",
  settingsConfig,
  category: "custom",
  ...extra,
});

const renderCard = (provider: Provider, appId: AppId) =>
  renderWithClient(
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
    />,
  );

describe("ProviderCard endpoint line (FE-PROV-09)", () => {
  it.each<[AppId, Record<string, unknown>, string]>([
    [
      "opencode",
      { options: { baseURL: "https://opencode.example.com/v1" } },
      "https://opencode.example.com/v1",
    ],
    [
      "openclaw",
      { baseUrl: "https://openclaw.example.com/v1" },
      "https://openclaw.example.com/v1",
    ],
    [
      "hermes",
      { base_url: "https://hermes.example.com/v1" },
      "https://hermes.example.com/v1",
    ],
    [
      "grokbuild",
      {
        config:
          '[models]\ndefault = "grok"\n\n[model.grok]\nbase_url = "https://grok.example.com/v1"\n',
      },
      "https://grok.example.com/v1",
    ],
  ])("%s shows its configured endpoint", (appId, config, url) => {
    expect(getProviderCardUrl(makeProvider(config), appId)).toBe(url);
    renderCard(makeProvider(config), appId);
    expect(screen.getByText(url)).toBeInTheDocument();
  });

  it("keeps the endpoint visible and shows notes on their own line", () => {
    renderCard(
      makeProvider(
        { env: { ANTHROPIC_BASE_URL: "https://relay.example.com" } },
        { notes: "team key" },
      ),
      "claude",
    );
    expect(screen.getByText("https://relay.example.com")).toBeInTheDocument();
    expect(screen.getByText("team key")).toBeInTheDocument();
  });

  it("prefers the website URL and falls back to 'not configured'", () => {
    expect(
      getProviderCardUrl(
        makeProvider(
          { options: { baseURL: "https://api.example.com" } },
          { websiteUrl: "https://site.example.com" },
        ),
        "opencode",
      ),
    ).toBe("https://site.example.com");
    expect(getProviderCardUrl(makeProvider({}), "hermes")).toBeUndefined();
  });
});

describe("ProviderCard health query (FE-PROV-13)", () => {
  it("does not query provider health while the proxy is stopped", async () => {
    const { callsTo } = mockInvoke({
      get_provider_health: { is_healthy: true, consecutive_failures: 0 },
    });
    renderCard(
      makeProvider({ env: { ANTHROPIC_BASE_URL: "https://a.example.com" } }),
      "claude",
    );
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(callsTo("get_provider_health")).toHaveLength(0);
  });
});
