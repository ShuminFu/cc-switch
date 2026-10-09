import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SessionStatsTable } from "@/components/usage/SessionStatsTable";
import type { UsageRangeSelection } from "@/types/usage";

const useTopSessionsMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/usage", () => ({
  useTopSessions: (...args: unknown[]) => useTopSessionsMock(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string } | string) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

const range = { preset: "today" } as UsageRangeSelection;

describe("SessionStatsTable", () => {
  it("passes the dashboard scope to the query and lists sessions with totals", () => {
    useTopSessionsMock.mockReturnValue({
      isLoading: false,
      data: [
        {
          sessionId: "sess-1",
          requests: 4,
          totalCostUsd: "1.234567",
          inputTokens: 100,
          outputTokens: 50,
          cacheReadTokens: 1000,
          cacheCreationTokens: 10,
          firstSeenAt: 1_800_000_000,
          lastSeenAt: 1_800_000_600,
        },
      ],
    });
    const onShowRequests = vi.fn();
    render(
      <SessionStatsTable
        range={range}
        appType="claude"
        providerName="Relay"
        refreshIntervalMs={0}
        onShowRequests={onShowRequests}
      />,
    );
    expect(useTopSessionsMock).toHaveBeenCalledWith(
      range,
      { appType: "claude", providerName: "Relay", model: undefined },
      { refetchInterval: false },
    );
    const row = screen.getByTestId("session-stat-sess-1");
    expect(row).toHaveTextContent("sess-1");
    expect(row).toHaveTextContent("1,160");
    expect(row).toHaveTextContent("$1.2346");

    fireEvent.click(screen.getByRole("button", { name: "查看该会话的请求" }));
    expect(onShowRequests).toHaveBeenCalledWith("sess-1");
  });

  it("shows the empty hint without data", () => {
    useTopSessionsMock.mockReturnValue({ isLoading: false, data: [] });
    render(<SessionStatsTable range={range} refreshIntervalMs={0} />);
    expect(screen.getByText("暂无数据")).toBeInTheDocument();
  });
});
