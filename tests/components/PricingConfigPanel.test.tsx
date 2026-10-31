import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

const stableT = vi.hoisted(() => (key: string) => key);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT }),
}));

const multipliers: Record<string, string> = {
  claude: "1",
  codex: "1.2",
  gemini: "1",
  grokbuild: "0.5",
  opencode: "2",
};

const baseResponders = {
  get_model_pricing: [
    {
      modelId: "claude-sonnet-4",
      displayName: "Claude Sonnet 4",
      inputCostPerMillion: "3",
      outputCostPerMillion: "15",
      cacheReadCostPerMillion: "0.3",
      cacheCreationCostPerMillion: "3.75",
    },
  ],
  get_default_cost_multiplier: ({ appType }: Record<string, unknown>) =>
    multipliers[appType as string],
  get_pricing_model_source: ({ appType }: Record<string, unknown>) =>
    appType === "codex" ? "request" : "response",
};

const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <PricingConfigPanel />
    </QueryClientProvider>,
  );

const multiplierInputs = () =>
  screen.getAllByPlaceholderText("1") as HTMLInputElement[];

beforeEach(() => {
  toastMock.error.mockReset();
  toastMock.success.mockReset();
});

describe("PricingConfigPanel", () => {
  it("loads per-app defaults and the model pricing table", async () => {
    mockInvoke(baseResponders);
    renderPanel();

    expect(await screen.findByText("claude-sonnet-4")).toBeInTheDocument();
    await waitFor(() =>
      expect(multiplierInputs().map((i) => i.value)).toEqual([
        "1",
        "1.2",
        "1",
        "0.5",
        "2",
      ]),
    );
    // OpenCode is priced too (FE-UI-29).
    expect(screen.getByTestId("pricing-app-opencode")).toBeInTheDocument();
    // Nothing changed yet, so there is nothing to save.
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  });

  it("saves only the changed app's multiplier and source", async () => {
    const api = mockInvoke({
      ...baseResponders,
      set_default_cost_multiplier: null,
      set_pricing_model_source: null,
    });
    renderPanel();

    await waitFor(() => expect(multiplierInputs()[1]).toHaveValue(1.2));
    fireEvent.change(multiplierInputs()[1], { target: { value: "1.5" } });
    const save = screen.getByRole("button", { name: "common.save" });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);

    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith(
        "settings.globalProxy.pricingSaved",
      ),
    );
    const multiplierCalls = api.callsTo("set_default_cost_multiplier");
    expect(multiplierCalls.map((c) => c.args)).toEqual([
      { appType: "codex", value: "1.5" },
    ]);
    expect(api.callsTo("set_pricing_model_source").map((c) => c.args)).toEqual([
      { appType: "codex", value: "request" },
    ]);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled(),
    );
  });

  it("rejects an invalid multiplier without calling the backend", async () => {
    const api = mockInvoke({
      ...baseResponders,
      set_default_cost_multiplier: null,
      set_pricing_model_source: null,
    });
    renderPanel();

    await waitFor(() => expect(multiplierInputs()[0]).toHaveValue(1));
    fireEvent.change(multiplierInputs()[0], { target: { value: "-2" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(api.callsTo("set_default_cost_multiplier")).toHaveLength(0);
  });

  it("reports which app failed on a partial save and keeps it dirty", async () => {
    const api = mockInvoke({
      ...baseResponders,
      set_default_cost_multiplier: ({ appType }: Record<string, unknown>) => {
        if (appType === "gemini") throw new Error("locked");
        return null;
      },
      set_pricing_model_source: null,
    });
    renderPanel();

    await waitFor(() => expect(multiplierInputs()[2]).toHaveValue(1));
    fireEvent.change(multiplierInputs()[1], { target: { value: "1.5" } });
    fireEvent.change(multiplierInputs()[2], { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(toastMock.success).not.toHaveBeenCalled();
    expect(api.callsTo("set_default_cost_multiplier")).toHaveLength(2);
    // Gemini is still dirty (and flagged), codex is saved.
    expect(screen.getByTestId("pricing-app-gemini")).toHaveTextContent(
      "settings.globalProxy.pricingSaveFailedBadge",
    );
    expect(screen.getByTestId("pricing-app-codex")).not.toHaveTextContent(
      "settings.globalProxy.pricingSaveFailedBadge",
    );
    expect(
      screen.getByRole("button", { name: "common.save" }),
    ).not.toBeDisabled();
  });

  it("shows an error instead of the table when pricing cannot be loaded", async () => {
    mockInvoke({
      ...baseResponders,
      get_model_pricing: () => {
        throw new Error("no such table: model_pricing");
      },
    });
    renderPanel();

    expect(
      await screen.findByText(/no such table: model_pricing/),
    ).toBeInTheDocument();
    expect(screen.queryByText("claude-sonnet-4")).toBeNull();
  });
});
