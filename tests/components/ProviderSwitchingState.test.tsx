import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { useProviderActions } from "@/hooks/useProviderActions";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

// The footers poll usage/quota endpoints the switch flow does not care about.
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({
  default: () => null,
}));

const createProvider = (id: string, sortIndex: number): Provider => ({
  id,
  name: `Provider ${id.toUpperCase()}`,
  settingsConfig: {},
  category: "custom",
  sortIndex,
});

const providers: Record<string, Provider> = {
  a: createProvider("a", 0),
  b: createProvider("b", 1),
  c: createProvider("c", 2),
};

function Harness() {
  const { switchProvider, switchingProviderId } = useProviderActions("claude");
  return (
    <ProviderList
      providers={providers}
      currentProviderId="a"
      appId="claude"
      onSwitch={(provider) => void switchProvider(provider)}
      switchingProviderId={switchingProviderId}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onDuplicate={vi.fn()}
      onOpenWebsite={vi.fn()}
    />
  );
}

const mainActionOf = (name: string): HTMLButtonElement => {
  const card = screen.getByText(name).closest(".group");
  if (!card) throw new Error(`card for ${name} not found`);
  const button = card.querySelector<HTMLButtonElement>(
    '[data-testid="provider-main-action"]',
  );
  if (!button) throw new Error(`main action for ${name} not found`);
  return button;
};

describe("provider switching state", () => {
  let switchCalls: string[];
  let resolveSwitch: (() => void) | undefined;

  beforeEach(() => {
    switchCalls = [];
    resolveSwitch = undefined;
    server.use(
      http.post(`${TAURI_ENDPOINT}/switch_provider`, async ({ request }) => {
        const { id } = (await request.json()) as { id: string };
        switchCalls.push(id);
        await new Promise<void>((resolve) => {
          resolveSwitch = resolve;
        });
        return HttpResponse.json({ warnings: [] });
      }),
      http.post(`${TAURI_ENDPOINT}/get_auto_failover_enabled`, () =>
        HttpResponse.json(false),
      ),
    );
  });

  it("disables the other Switch buttons until the pending switch resolves", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <Harness />
      </QueryClientProvider>,
    );

    // Idle: every non-current card can be switched to.
    expect(mainActionOf("Provider A")).toBeDisabled();
    expect(mainActionOf("Provider B")).toBeEnabled();
    expect(mainActionOf("Provider C")).toBeEnabled();

    fireEvent.click(mainActionOf("Provider B"));

    await waitFor(() => expect(switchCalls).toEqual(["b"]));
    await waitFor(() =>
      expect(mainActionOf("Provider B")).toHaveAttribute("aria-busy", "true"),
    );
    expect(mainActionOf("Provider B")).toBeDisabled();
    expect(mainActionOf("Provider C")).toBeDisabled();
    expect(mainActionOf("Provider C")).not.toHaveAttribute("aria-busy");

    // A second click while pending must not start another switch.
    fireEvent.click(mainActionOf("Provider C"));
    expect(switchCalls).toEqual(["b"]);

    resolveSwitch?.();

    await waitFor(() => expect(mainActionOf("Provider C")).toBeEnabled());
    expect(mainActionOf("Provider B")).not.toHaveAttribute("aria-busy");
    expect(switchCalls).toEqual(["b"]);
  });
});

describe("ProviderActions while a switch is pending", () => {
  const baseProps = {
    onSwitch: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
  };

  it("leaves actions that do not switch enabled", () => {
    const onToggleFailover = vi.fn();
    render(
      <ProviderActions
        {...baseProps}
        appId="claude"
        isCurrent={false}
        isAutoFailoverEnabled
        onToggleFailover={onToggleFailover}
        isSwitchPending
      />,
    );
    const button = screen.getByTestId("provider-main-action");
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onToggleFailover).toHaveBeenCalledWith(true);
  });

  it("disables add-to-config in additive mode but not remove", () => {
    const { rerender } = render(
      <ProviderActions
        {...baseProps}
        appId="opencode"
        isCurrent={false}
        isInConfig={false}
        isSwitchPending
      />,
    );
    expect(screen.getByTestId("provider-main-action")).toBeDisabled();

    rerender(
      <ProviderActions
        {...baseProps}
        appId="opencode"
        isCurrent={false}
        isInConfig
        onRemoveFromConfig={vi.fn()}
        isSwitchPending
      />,
    );
    expect(screen.getByTestId("provider-main-action")).toBeEnabled();
  });

  it("disables the Hermes enable button and marks the switching card busy", () => {
    render(
      <ProviderActions
        {...baseProps}
        appId="hermes"
        isCurrent={false}
        isInConfig
        onRemoveFromConfig={vi.fn()}
        onSetAsDefault={vi.fn()}
        isSwitching
        isSwitchPending
      />,
    );
    const enable = screen.getByRole("button", { name: "provider.enable" });
    expect(enable).toBeDisabled();
    expect(enable).toHaveAttribute("aria-busy", "true");
  });

  it("labels the OpenClaw default button and keeps it usable during a switch", () => {
    const onSetAsDefault = vi.fn();
    render(
      <ProviderActions
        {...baseProps}
        appId="openclaw"
        isCurrent={false}
        isInConfig
        onRemoveFromConfig={vi.fn()}
        onSetAsDefault={onSetAsDefault}
        isSwitchPending
      />,
    );
    const setDefault = screen.getByRole("button", {
      name: "provider.setAsDefault",
    });
    expect(setDefault).toBeEnabled();
    fireEvent.click(setDefault);
    expect(onSetAsDefault).toHaveBeenCalled();
    // Additive mode: in-config providers offer remove, not switch
    expect(screen.getByTestId("provider-main-action")).toBeEnabled();
  });

  it("shows no default button for apps without a default selection", () => {
    render(
      <ProviderActions
        {...baseProps}
        appId="opencode"
        isCurrent={false}
        isInConfig
        onRemoveFromConfig={vi.fn()}
        onSetAsDefault={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "provider.setAsDefault" }),
    ).toBeNull();
  });
});
