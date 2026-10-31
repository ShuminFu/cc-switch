import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { AppId } from "@/lib/api";
import type { ComponentProps } from "react";
import {
  ProvidersHome,
  type ProvidersHomeProps,
} from "@/components/app-shell/ProvidersHome";
import type { ProviderList } from "@/components/providers/ProviderList";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";

const providerListProps = vi.hoisted(
  () => [] as Array<ComponentProps<typeof ProviderList>>,
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: (props: ComponentProps<typeof ProviderList>) => {
    providerListProps.push(props);
    return null;
  },
}));

const shellActions = {
  disableOmo: vi.fn(),
  disableOmoSlim: vi.fn(),
  duplicateProvider: vi.fn(),
  openWebsite: vi.fn(),
  openTerminal: vi.fn(),
};

const renderHome = (activeApp: AppId) => {
  const onSwitch = vi.fn();
  const onSetAsDefaultModel = vi.fn();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <ProvidersHome
        activeApp={activeApp}
        providers={{}}
        currentProviderId=""
        isLoading={false}
        isProxyRunning={false}
        isProxyTakeover={false}
        onSwitch={onSwitch}
        onSetAsDefaultModel={onSetAsDefaultModel}
        shellActions={
          shellActions as unknown as ProvidersHomeProps["shellActions"]
        }
        onEdit={vi.fn()}
        onConfigureUsage={vi.fn()}
        onCreate={vi.fn()}
        onRequestConfirm={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const props = providerListProps[providerListProps.length - 1];
  return { props, onSwitch, onSetAsDefaultModel };
};

// 每个应用传给供应商列表的可选操作（取代 ProvidersHome 中的 app-id 判断）
describe("ProvidersHome per-app list actions", () => {
  beforeEach(() => {
    providerListProps.length = 0;
  });

  it.each<[AppId, boolean, boolean, boolean, "switch" | "model" | null]>([
    ["claude", false, false, true, null],
    ["claude-desktop", false, false, false, null],
    ["codex", false, false, false, null],
    ["gemini", false, false, false, null],
    ["grokbuild", false, false, false, null],
    ["opencode", true, true, false, null],
    ["openclaw", true, false, false, "model"],
    ["hermes", true, false, false, "switch"],
  ])(
    "%s: remove=%s omo=%s terminal=%s setDefault=%s",
    (appId, remove, omo, terminal, setDefault) => {
      const { props, onSwitch, onSetAsDefaultModel } = renderHome(appId);
      expect(props.appId).toBe(appId);
      expect(props.onRemoveFromConfig !== undefined).toBe(remove);
      expect(props.onDisableOmo).toBe(
        omo ? shellActions.disableOmo : undefined,
      );
      expect(props.onDisableOmoSlim).toBe(
        omo ? shellActions.disableOmoSlim : undefined,
      );
      expect(props.onOpenTerminal).toBe(
        terminal ? shellActions.openTerminal : undefined,
      );
      expect(props.onSetAsDefault).toBe(
        setDefault === "switch"
          ? onSwitch
          : setDefault === "model"
            ? onSetAsDefaultModel
            : undefined,
      );
    },
  );
});

describe("ProviderEmptyState per-app text", () => {
  const snippetHint = "provider.noProvidersDescriptionSnippet";

  it.each<[AppId, boolean, string]>([
    ["claude", true, "provider.importCurrent"],
    ["claude-desktop", false, "provider.importFromClaude"],
    ["codex", true, "provider.importCurrent"],
    ["gemini", true, "provider.importCurrent"],
    ["grokbuild", false, "provider.importCurrent"],
    ["opencode", false, "provider.importCurrent"],
    ["openclaw", false, "provider.importCurrent"],
    ["hermes", false, "provider.importCurrent"],
  ])("%s: snippet hint=%s, import label %s", (appId, hint, importLabel) => {
    render(
      <ProviderEmptyState
        appId={appId}
        onImport={vi.fn()}
        onCreate={vi.fn()}
      />,
    );
    expect(screen.queryByText(snippetHint) !== null).toBe(hint);
    expect(
      screen.getByRole("button", { name: importLabel }),
    ).toBeInTheDocument();
  });
});
