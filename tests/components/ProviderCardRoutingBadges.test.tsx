import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => null,
}));

const BADGE_KEYS = [
  "claudeDesktop.modeProxy",
  "claudeCode.needsRouting",
  "claudeCode.noRoutingSupport",
  "codex.needsRouting",
  "codex.noRoutingSupport",
  "codex.officialRouting",
  "codex.nativeLogin",
];

function renderCard(
  appId: AppId,
  provider: Partial<Provider>,
  isProxyTakeover = false,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={{
          id: "p1",
          name: "Provider",
          settingsConfig: {},
          ...provider,
        }}
        isCurrent={false}
        appId={appId}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
        isProxyRunning={false}
        isProxyTakeover={isProxyTakeover}
      />
    </QueryClientProvider>,
  );
}

/** 卡片上渲染出的路由徽标（测试环境 i18n 无资源，t() 原样返回 key） */
const renderedBadges = () =>
  BADGE_KEYS.filter((key) => screen.queryByText(key) !== null);

const codexChatConfig = 'model_provider = "x"\nwire_api = "chat"\n';

// 每个用例对应重构前 ProviderCard 中逐个 appId 判断的徽标条件
describe("ProviderCard routing badges", () => {
  it.each<[string, AppId, Partial<Provider>, boolean, string[]]>([
    [
      "Claude Desktop proxy mode",
      "claude-desktop",
      { meta: { claudeDesktopMode: "proxy" } },
      false,
      ["claudeDesktop.modeProxy"],
    ],
    [
      "official Claude Desktop in proxy mode",
      "claude-desktop",
      { category: "official", meta: { claudeDesktopMode: "proxy" } },
      false,
      [],
    ],
    [
      "Claude OpenAI-format provider",
      "claude",
      { meta: { apiFormat: "openai_chat" } },
      false,
      ["claudeCode.needsRouting"],
    ],
    [
      "Claude Anthropic-format provider",
      "claude",
      { meta: { apiFormat: "anthropic" } },
      false,
      [],
    ],
    [
      "official Claude provider",
      "claude",
      { category: "official", meta: { apiFormat: "openai_chat" } },
      false,
      ["claudeCode.noRoutingSupport"],
    ],
    [
      "Codex chat wire_api provider",
      "codex",
      { settingsConfig: { config: codexChatConfig } },
      false,
      ["codex.needsRouting"],
    ],
    [
      "Codex Anthropic-format provider",
      "codex",
      { meta: { apiFormat: "anthropic" } },
      false,
      ["codex.needsRouting"],
    ],
    [
      "official Codex provider without official routing",
      "codex",
      { category: "official", meta: { apiFormat: "openai_chat" } },
      false,
      ["codex.noRoutingSupport"],
    ],
    [
      "Codex official account (native login)",
      "codex",
      { id: CODEX_OFFICIAL_PROVIDER_ID, category: "official" },
      false,
      ["codex.nativeLogin"],
    ],
    [
      "Codex official account under proxy takeover",
      "codex",
      { id: CODEX_OFFICIAL_PROVIDER_ID, category: "official" },
      true,
      ["codex.officialRouting"],
    ],
    [
      "Grok Build OpenAI-format provider",
      "grokbuild",
      { meta: { apiFormat: "openai_chat" } },
      false,
      [],
    ],
    [
      "Gemini OpenAI-format provider",
      "gemini",
      { meta: { apiFormat: "openai_chat" } },
      false,
      [],
    ],
  ])("%s", (_name, appId, provider, isProxyTakeover, expected) => {
    renderCard(appId, provider, isProxyTakeover);
    expect(renderedBadges()).toEqual(expected);
  });
});
