import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderSpendLimitBadge } from "@/components/providers/ProviderSpendLimitBadge";
import type { ProviderLimitStatus } from "@/types/usage";

const base: ProviderLimitStatus = {
  providerId: "p1",
  dailyUsage: "3.2",
  dailyLimit: "10.00",
  dailyExceeded: false,
  monthlyUsage: "40",
  monthlyLimit: "100.00",
  monthlyExceeded: false,
};

describe("ProviderSpendLimitBadge", () => {
  it("renders nothing when no limit is configured", () => {
    const { container } = render(
      <ProviderSpendLimitBadge
        status={{ ...base, dailyLimit: undefined, monthlyLimit: undefined }}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows today's spend against the daily limit with both windows in the tooltip", () => {
    render(<ProviderSpendLimitBadge status={base} />);

    const badge = screen.getByTestId("spend-limit-badge");
    expect(badge).toHaveTextContent("$3.20 / $10.00");
    expect(badge.getAttribute("title")).toContain("今日 $3.20 / $10.00");
    expect(badge.getAttribute("title")).toContain("本月 $40.00 / $100.00");
    expect(badge.className).toContain("emerald");
  });

  it("turns yellow near the limit and falls back to the monthly window", () => {
    render(
      <ProviderSpendLimitBadge
        status={{
          ...base,
          dailyLimit: undefined,
          monthlyUsage: "75",
        }}
      />,
    );

    const badge = screen.getByTestId("spend-limit-badge");
    expect(badge).toHaveTextContent("$75.00 / $100.00");
    expect(badge.className).toContain("yellow");
  });

  it("flags an exceeded budget in red", () => {
    render(
      <ProviderSpendLimitBadge
        status={{ ...base, dailyUsage: "12.5", dailyExceeded: true }}
      />,
    );

    const badge = screen.getByTestId("spend-limit-badge");
    expect(badge).toHaveTextContent("超支");
    expect(badge).toHaveTextContent("$12.50 / $10.00");
    expect(badge.className).toContain("red");
    expect(badge.getAttribute("title")).toContain("已超出支出上限");
  });
});
