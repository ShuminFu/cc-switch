import { render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BudgetHeroStrip } from "@/components/usage/BudgetHeroStrip";
import { resetBudgetExceededAlerts } from "@/components/usage/budgetLimits";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  warning: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

const providers = {
  limited: {
    id: "limited",
    name: "Relay",
    settingsConfig: {},
    meta: { limitDailyUsd: "1" },
  },
  plain: { id: "plain", name: "Plain", settingsConfig: {} },
};

const renderStrip = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <BudgetHeroStrip appType="claude" />
    </QueryClientProvider>,
  );

describe("BudgetHeroStrip (FE-UI-26)", () => {
  beforeEach(() => {
    resetBudgetExceededAlerts();
    toastMock.warning.mockReset();
  });

  it("shows the exceeded state and warns once when today's spend is over the daily budget", async () => {
    mockInvoke({
      get_providers: providers,
      get_current_provider: "limited",
      check_provider_limits: {
        providerId: "limited",
        dailyUsage: "1.50",
        dailyLimit: "1",
        dailyExceeded: true,
        monthlyUsage: "1.50",
        monthlyLimit: null,
        monthlyExceeded: false,
      },
    });
    const first = renderStrip();

    const chip = await screen.findByTestId("budget-chip-limited");
    await waitFor(() => expect(chip).toHaveAttribute("data-level", "exceeded"));
    expect(chip).toHaveTextContent("$1.50 / $1.00");
    expect(screen.queryByTestId("budget-chip-plain")).toBeNull();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));

    // Remounting (e.g. a tab switch) does not repeat the alert.
    first.unmount();
    renderStrip();
    await screen.findByTestId("budget-chip-limited");
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
  });
});
