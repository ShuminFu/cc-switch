import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";

describe("ProviderHealthBadge", () => {
  // The test i18n instance has no resources, so components render their
  // defaultValue fallbacks (Chinese).
  it("renders the operational state without a reset control", () => {
    render(<ProviderHealthBadge consecutiveFailures={0} isHealthy />);

    expect(screen.getByText("正常")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows breaker details in the tooltip and resets on click", () => {
    const onReset = vi.fn();
    render(
      <ProviderHealthBadge
        consecutiveFailures={4}
        isHealthy={false}
        stats={{
          state: "open",
          consecutiveFailures: 4,
          consecutiveSuccesses: 0,
          totalRequests: 9,
          failedRequests: 4,
          retryAfterSeconds: 42,
        }}
        lastError="upstream 503"
        onReset={onReset}
      />,
    );

    const badge = screen.getByText("熔断").parentElement;
    expect(badge).not.toBeNull();
    const tooltip = badge!.getAttribute("title") ?? "";
    expect(tooltip).toContain("连续失败 4 次");
    expect(tooltip).toContain("熔断器：已熔断");
    expect(tooltip).toContain("42 秒后再次探测");
    expect(tooltip).toContain("最近错误：upstream 503");

    const reset = screen.getByRole("button", { name: "重置熔断器" });
    fireEvent.click(reset);
    expect(onReset).toHaveBeenCalledTimes(1);
  });

  it("hides the reset control while the provider is only degraded", () => {
    render(
      <ProviderHealthBadge
        consecutiveFailures={2}
        isHealthy
        onReset={() => {}}
      />,
    );

    expect(screen.getByText("降级")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("disables the reset control while a reset is pending", () => {
    render(
      <ProviderHealthBadge
        consecutiveFailures={5}
        isHealthy={false}
        onReset={() => {}}
        isResetting
      />,
    );

    expect(screen.getByRole("button", { name: "重置熔断器" })).toBeDisabled();
  });
});
