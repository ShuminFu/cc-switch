import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { server } from "../msw/server";
import {
  applyProviderLimits,
  computeBudgetProgress,
  getProviderLimits,
  normalizeLimitInput,
  parseLimit,
  worstBudgetProgress,
} from "@/components/usage/budgetLimits";
import { BudgetLimitBadge } from "@/components/usage/BudgetLimitBadge";
import { BudgetLimitRow } from "@/components/usage/BudgetLimitsPanel";

const TAURI = "http://tauri.local";

const baseProvider: Provider = {
  id: "p1",
  name: "Relay",
  settingsConfig: {},
  meta: { isPartner: true },
} as Provider;

const wrap = (ui: ReactNode) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{ui}</QueryClientProvider>,
  );
};

describe("budget limit helpers", () => {
  it("parses only positive limits", () => {
    expect(parseLimit("1.5")).toBe(1.5);
    expect(parseLimit(" 2 ")).toBe(2);
    expect(parseLimit("0")).toBeNull();
    expect(parseLimit("-1")).toBeNull();
    expect(parseLimit("abc")).toBeNull();
    expect(parseLimit(undefined)).toBeNull();
  });

  it("normalizes user input", () => {
    expect(normalizeLimitInput("")).toBeUndefined();
    expect(normalizeLimitInput("  ")).toBeUndefined();
    expect(normalizeLimitInput("10")).toBe("10");
    expect(normalizeLimitInput(" 2.50 ")).toBe("2.50");
    expect(normalizeLimitInput("0")).toBeNull();
    expect(normalizeLimitInput("1e3")).toBeNull();
    expect(normalizeLimitInput("-3")).toBeNull();
  });

  it("writes and clears meta keys without touching other meta", () => {
    const withLimits = applyProviderLimits(baseProvider, {
      daily: "1",
      monthly: "20",
    });
    expect(withLimits.meta).toMatchObject({
      isPartner: true,
      limitDailyUsd: "1",
      limitMonthlyUsd: "20",
    });
    expect(getProviderLimits(withLimits)).toEqual({
      daily: "1",
      monthly: "20",
    });

    const cleared = applyProviderLimits(withLimits, { daily: "1" });
    expect(cleared.meta).not.toHaveProperty("limitMonthlyUsd");
    expect(cleared.meta).toMatchObject({ isPartner: true, limitDailyUsd: "1" });
    // the input provider is not mutated
    expect(withLimits.meta).toHaveProperty("limitMonthlyUsd", "20");
  });

  it("computes progress levels", () => {
    expect(computeBudgetProgress("0.5", "1.00")?.level).toBe("ok");
    expect(computeBudgetProgress("0.85", "1.00")?.level).toBe("warn");
    const over = computeBudgetProgress("1.5", "1.00");
    expect(over?.level).toBe("exceeded");
    expect(over?.percent).toBe(100);
    expect(computeBudgetProgress("1", undefined)).toBeNull();
  });

  it("picks the most severe period", () => {
    const worst = worstBudgetProgress({
      providerId: "p1",
      dailyUsage: "0.1",
      dailyLimit: "1.00",
      dailyExceeded: false,
      monthlyUsage: "19",
      monthlyLimit: "20.00",
      monthlyExceeded: false,
    });
    expect(worst?.period).toBe("monthly");
    expect(worst?.progress.level).toBe("warn");
  });
});

describe("BudgetLimitBadge", () => {
  it("renders nothing without limits", () => {
    const { container } = wrap(
      <BudgetLimitBadge provider={baseProvider} appId="claude" />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows an exceeded pill when over the daily limit", async () => {
    server.use(
      http.post(`${TAURI}/check_provider_limits`, () =>
        HttpResponse.json({
          providerId: "p1",
          dailyUsage: "1.500000",
          dailyLimit: "1.00",
          dailyExceeded: true,
          monthlyUsage: "1.500000",
          monthlyExceeded: false,
        }),
      ),
    );
    wrap(
      <BudgetLimitBadge
        provider={applyProviderLimits(baseProvider, { daily: "1" })}
        appId="claude"
      />,
    );
    const badge = await screen.findByTestId("budget-limit-badge");
    expect(badge).toHaveAttribute("data-level", "exceeded");
    expect(badge).toHaveTextContent("$1.50 / $1.00");
  });
});

describe("BudgetLimitRow", () => {
  it("saves limits into provider meta via update_provider", async () => {
    let saved: any = null;
    server.use(
      http.post(`${TAURI}/update_provider`, async ({ request }) => {
        saved = await request.json();
        return HttpResponse.json(true);
      }),
    );
    wrap(<BudgetLimitRow provider={baseProvider} appId="claude" />);

    const [daily, monthly] = screen.getAllByRole("textbox");
    const save = screen.getByRole("button", { name: /save/i });
    expect(save).toBeDisabled();

    fireEvent.change(daily, { target: { value: "abc" } });
    expect(save).toBeDisabled();

    fireEvent.change(daily, { target: { value: "1" } });
    fireEvent.change(monthly, { target: { value: "25" } });
    expect(save).toBeEnabled();
    fireEvent.click(save);

    await waitFor(() => expect(saved).not.toBeNull());
    expect(saved.app).toBe("claude");
    expect(saved.provider.meta).toMatchObject({
      isPartner: true,
      limitDailyUsd: "1",
      limitMonthlyUsd: "25",
    });
  });
});
