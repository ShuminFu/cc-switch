import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { resetDashboardViewState } from "@/components/usage/dashboardViewState";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
  };
});

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: () => <div data-testid="usage-hero" />,
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend" />,
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageExportButton", () => ({
  UsageExportButton: ({ appType }: { appType?: string }) => (
    <div data-testid="usage-export">{appType}</div>
  ),
}));

vi.mock("@/components/usage/BudgetHeroStrip", () => ({
  BudgetHeroStrip: () => <div data-testid="budget-hero-strip" />,
}));

vi.mock("@/components/usage/BudgetLimitsPanel", () => ({
  BudgetLimitsPanel: () => <div data-testid="budget-limits-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">date-range</button>,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <div data-testid={`select-${value}`}>
      {children}
      <button type="button" onClick={() => onValueChange?.("5000")}>
        choose-5000
      </button>
    </div>
  ),
  SelectTrigger: ({ children, ...props }: any) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <div>{children}</div>,
  SelectItem: ({ children, ...props }: any) => <div {...props}>{children}</div>,
}));

const renderDashboard = (props: ComponentProps<typeof UsageDashboard> = {}) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard {...props} />
    </QueryClientProvider>,
  );
};

describe("UsageDashboard", () => {
  beforeEach(() => {
    resetDashboardViewState();
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it("uses the saved refresh interval when mounted", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("persists refresh interval changes", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(true);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("rolls back optimistic interval changes when persistence fails", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(false);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    await waitFor(() =>
      expect(screen.getByTestId("select-30000")).toBeInTheDocument(),
    );
  });

  it("keeps the app filter after the dashboard remounts (FE-UI-31)", () => {
    const first = renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: "Codex" }));
    expect(screen.getByTestId("usage-export")).toHaveTextContent("codex");
    first.unmount();

    renderDashboard();
    expect(screen.getByTestId("usage-export")).toHaveTextContent("codex");
  });
});
