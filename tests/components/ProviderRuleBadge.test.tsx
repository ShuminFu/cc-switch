import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderRuleBadge } from "@/components/providers/ProviderRuleBadge";

const cancelMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/switchRules", () => ({
  switchRulesApi: { cancelRevert: cancelMock },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

describe("ProviderRuleBadge", () => {
  it("explains the automatic switch and cancels the revert on request", async () => {
    cancelMock.mockResolvedValue(true);
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <ProviderRuleBadge
          state={{
            ruleId: "r1",
            firedAt: 1_800_000_000,
            resetsAt: 1_800_010_000,
            switchedFrom: "official",
            switchedTo: "backup",
            lastUtilization: 92,
            reason: "five_hour 92%",
          }}
        />
      </QueryClientProvider>,
    );
    const badge = screen.getByTestId("provider-rule-badge");
    expect(badge).toHaveTextContent("规则切换");
    expect(badge.getAttribute("title")).toContain("five_hour 92%");
    expect(badge.getAttribute("title")).toContain("切回");

    fireEvent.click(screen.getByRole("button", { name: "保持当前" }));
    await waitFor(() => expect(cancelMock).toHaveBeenCalledWith("r1"));
  });
});
