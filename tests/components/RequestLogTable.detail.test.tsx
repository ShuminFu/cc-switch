import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { RequestLog } from "@/types/usage";

const log: RequestLog = {
  requestId: "req-42",
  providerId: "p1",
  providerName: "Provider One",
  appType: "claude",
  model: "claude-sonnet",
  costMultiplier: "1",
  inputTokens: 100,
  outputTokens: 50,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  inputCostUsd: "0.001",
  outputCostUsd: "0.002",
  cacheReadCostUsd: "0",
  cacheCreationCostUsd: "0",
  totalCostUsd: "0.003",
  isStreaming: false,
  latencyMs: 321,
  statusCode: 500,
  errorMessage: "upstream exploded",
  createdAt: 1_710_000_000,
};

const useRequestDetailMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { defaultValue?: string }) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: () => ({
    data: { data: [log], total: 1, page: 0, pageSize: 20 },
    isLoading: false,
  }),
  useRequestDetail: (id: string) => useRequestDetailMock(id),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div role="dialog">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ children }: any) => <div>{children}</div>,
  SelectTrigger: ({ children }: any) => <div>{children}</div>,
  SelectValue: () => null,
  SelectContent: () => null,
  SelectItem: () => null,
}));

describe("RequestLogTable request drill-down", () => {
  it("opens the request detail dialog when a row is clicked", () => {
    useRequestDetailMock.mockReturnValue({ data: log, isLoading: false });
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("Provider One"));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(useRequestDetailMock).toHaveBeenCalledWith("req-42");
    expect(screen.getByText("req-42")).toBeInTheDocument();
    expect(screen.getByText("upstream exploded")).toBeInTheDocument();
  });

  it("opens the detail from the keyboard", () => {
    useRequestDetailMock.mockReturnValue({ data: log, isLoading: false });
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );
    const row = screen.getByText("Provider One").closest("tr")!;
    fireEvent.keyDown(row, { key: "Enter" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
