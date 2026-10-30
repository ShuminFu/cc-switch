import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CircuitBreakerBadge } from "@/components/proxy/CircuitBreakerBadge";
import type { CircuitBreakerStats, CircuitProbeResult } from "@/types/proxy";

const mocks = vi.hoisted(() => ({
  stats: null as CircuitBreakerStats | null,
  probe: vi.fn(),
  isPending: false,
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key} ${JSON.stringify(options)}` : key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
    info: mocks.toastInfo,
  },
}));

vi.mock("@/lib/query/failover", () => ({
  useCircuitBreakerStats: () => ({
    data: mocks.stats,
    dataUpdatedAt: Date.now(),
  }),
  useProbeCircuitBreaker: () => ({
    mutateAsync: mocks.probe,
    isPending: mocks.isPending,
  }),
}));

const stats = (
  overrides: Partial<CircuitBreakerStats>,
): CircuitBreakerStats => ({
  state: "closed",
  consecutiveFailures: 0,
  consecutiveSuccesses: 0,
  totalRequests: 0,
  failedRequests: 0,
  openRemainingSecs: null,
  ...overrides,
});

const renderBadge = () =>
  render(<CircuitBreakerBadge providerId="p1" appType="claude" />);

describe("CircuitBreakerBadge", () => {
  beforeEach(() => {
    mocks.stats = null;
    mocks.isPending = false;
  });

  it("renders nothing while the proxy has no breaker for the provider", () => {
    const { container } = renderBadge();
    expect(container).toBeEmptyDOMElement();
  });

  it("renders a closed badge without a probe action", () => {
    mocks.stats = stats({ state: "closed" });
    renderBadge();

    const badge = screen.getByRole("status");
    expect(badge).toHaveAttribute("data-state", "closed");
    expect(badge).toHaveTextContent("health.circuitClosed");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("renders the open state with the time remaining and a probe action", () => {
    mocks.stats = stats({ state: "open", openRemainingSecs: 42 });
    renderBadge();

    const badge = screen.getByRole("status");
    expect(badge).toHaveAttribute("data-state", "open");
    expect(badge).toHaveTextContent(
      'health.circuitOpenRemaining {"seconds":42}',
    );
    expect(
      screen.getByRole("button", { name: /health\.probeNow/ }),
    ).toBeEnabled();
  });

  it("renders the half-open state with a probe action", () => {
    mocks.stats = stats({ state: "half_open" });
    renderBadge();

    expect(screen.getByRole("status")).toHaveAttribute(
      "data-state",
      "half_open",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "health.circuitHalfOpen",
    );
    expect(
      screen.getByRole("button", { name: /health\.probeNow/ }),
    ).toBeInTheDocument();
  });

  it("probes the provider and reports a recovered breaker", async () => {
    mocks.stats = stats({ state: "open", openRemainingSecs: 30 });
    const result: CircuitProbeResult = {
      probed: true,
      success: true,
      message: null,
      stats: stats({ state: "closed" }),
    };
    mocks.probe.mockResolvedValue(result);
    renderBadge();

    fireEvent.click(screen.getByRole("button", { name: /health\.probeNow/ }));

    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith("health.probeSucceeded"),
    );
    expect(mocks.probe).toHaveBeenCalledWith({
      providerId: "p1",
      appType: "claude",
    });
  });

  it("reports a failed probe", async () => {
    mocks.stats = stats({ state: "open", openRemainingSecs: 30 });
    mocks.probe.mockResolvedValue({
      probed: true,
      success: false,
      message: "Upstream HTTP 503",
      stats: stats({ state: "open", openRemainingSecs: 60 }),
    } satisfies CircuitProbeResult);
    renderBadge();

    fireEvent.click(screen.getByRole("button", { name: /health\.probeNow/ }));

    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith(
        'health.probeFailed {"detail":"Upstream HTTP 503"}',
      ),
    );
  });

  it("disables the probe action while a probe is in flight", () => {
    mocks.stats = stats({ state: "half_open" });
    mocks.isPending = true;
    renderBadge();

    expect(
      screen.getByRole("button", { name: /health\.probing/ }),
    ).toBeDisabled();
  });
});
