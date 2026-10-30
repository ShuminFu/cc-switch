import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { UsageRangeSelection } from "@/types/usage";

const useRequestLogsMock = vi.hoisted(() => vi.fn());
const captureStatusMock = vi.hoisted(() => vi.fn());
const toggleCaptureMock = vi.hoisted(() => vi.fn());
const exportBugReportMock = vi.hoisted(() => vi.fn());
const traceQueryMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: {
        defaultValue?: string;
        minutes?: number;
      },
    ) =>
      options?.minutes != null
        ? `${key}(${options.minutes})`
        : (options?.defaultValue ?? key),
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

vi.mock("@/lib/query/proxy", () => ({
  BUG_REPORT_CAPTURE_MINUTES: 10,
  useProxyCaptureStatus: () => ({ data: captureStatusMock() }),
  useToggleProxyCapture: () => ({
    mutate: toggleCaptureMock,
    isPending: false,
  }),
  useExportBugReport: () => ({
    mutate: exportBugReportMock,
    isPending: false,
  }),
  useProxyRequestTrace: (requestId: string | null) => traceQueryMock(requestId),
}));

type MockProps = { open?: boolean; children?: ReactNode };

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: MockProps) =>
    open ? <div>{children}</div> : null,
  DialogContent: ({ children }: MockProps) => <div>{children}</div>,
  DialogHeader: ({ children }: MockProps) => <div>{children}</div>,
  DialogTitle: ({ children }: MockProps) => <h2>{children}</h2>,
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => (
    <button {...props}>{children}</button>
  ),
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ children }: any) => <div>{children}</div>,
  SelectTrigger: ({ children, ...props }: any) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
  SelectValue: ({ placeholder }: any) => <span>{placeholder ?? null}</span>,
  SelectContent: () => null,
  SelectItem: () => null,
}));

vi.mock("@/components/ui/table", () => ({
  Table: ({ children }: any) => <table>{children}</table>,
  TableBody: ({ children }: any) => <tbody>{children}</tbody>,
  TableCell: ({ children, ...props }: any) => <td {...props}>{children}</td>,
  TableHead: ({ children, ...props }: any) => <th {...props}>{children}</th>,
  TableHeader: ({ children }: any) => <thead>{children}</thead>,
  TableRow: ({ children }: any) => <tr>{children}</tr>,
}));

describe("RequestLogTable", () => {
  beforeEach(() => {
    captureStatusMock.mockReset();
    captureStatusMock.mockReturnValue({
      active: false,
      expiresAt: null,
      remainingSecs: 0,
      capturedRequestIds: [],
    });
    toggleCaptureMock.mockReset();
    exportBugReportMock.mockReset();
    traceQueryMock.mockReset();
    traceQueryMock.mockReturnValue({ data: null, isLoading: false });
    useRequestLogsMock.mockReset();
    useRequestLogsMock.mockImplementation(
      ({ page = 0, pageSize = 20 }: { page?: number; pageSize?: number }) => ({
        data: {
          data: [],
          total: 120,
          page,
          pageSize,
        },
        isLoading: false,
      }),
    );
  });

  it("resets pagination when the dashboard range changes", async () => {
    const initialRange: UsageRangeSelection = { preset: "today" };
    const nextRange: UsageRangeSelection = {
      preset: "custom",
      customStartDate: 1_710_000_000,
      customEndDate: 1_710_086_400,
    };

    const { rerender } = render(
      <RequestLogTable
        range={initialRange}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "2" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range: initialRange,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={nextRange}
        rangeLabel="Custom"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range: nextRange,
        }),
      );
    });
  });

  it("resets pagination when the dashboard app filter changes", async () => {
    const range: UsageRangeSelection = { preset: "today" };
    const { rerender } = render(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "2" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="claude"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range,
        }),
      );
    });
  });

  describe("bug report capture (CCS-138)", () => {
    const logRow = (requestId: string) => ({
      requestId,
      providerId: "p1",
      providerName: "Provider",
      appType: "claude",
      model: "gpt-4o",
      requestModel: "claude-sonnet-4-5",
      costMultiplier: "1",
      inputTokens: 10,
      outputTokens: 2,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      inputCostUsd: "0",
      outputCostUsd: "0",
      cacheReadCostUsd: "0",
      cacheCreationCostUsd: "0",
      totalCostUsd: "0.0001",
      isStreaming: true,
      latencyMs: 1200,
      firstTokenMs: 300,
      statusCode: 200,
      createdAt: 1_710_000_000,
      dataSource: "proxy",
    });

    const renderTable = () =>
      render(
        <RequestLogTable
          range={{ preset: "today" }}
          rangeLabel="Today"
          appType="all"
          refreshIntervalMs={0}
        />,
      );

    it("arms capture from the toolbar", () => {
      renderTable();
      const toggle = screen.getByRole("button", {
        name: "usage.bugReport.captureStart",
      });
      expect(toggle).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(toggle);
      expect(toggleCaptureMock).toHaveBeenCalledWith(true);
    });

    it("shows the remaining time and stops an active capture", () => {
      captureStatusMock.mockReturnValue({
        active: true,
        expiresAt: Date.now() + 240_000,
        remainingSecs: 230,
        capturedRequestIds: [],
      });
      renderTable();
      const toggle = screen.getByRole("button", {
        name: "usage.bugReport.captureActive(4)",
      });
      expect(toggle).toHaveAttribute("aria-pressed", "true");
      fireEvent.click(toggle);
      expect(toggleCaptureMock).toHaveBeenCalledWith(false);
    });

    it("offers Export bug report only on captured rows", () => {
      useRequestLogsMock.mockReturnValue({
        data: {
          data: [logRow("req-captured"), logRow("req-plain")],
          total: 2,
          page: 0,
          pageSize: 20,
        },
        isLoading: false,
      });
      captureStatusMock.mockReturnValue({
        active: false,
        expiresAt: null,
        remainingSecs: 0,
        capturedRequestIds: ["req-captured"],
      });
      renderTable();

      const exportButtons = screen.getAllByRole("button", {
        name: "usage.bugReport.export",
      });
      expect(exportButtons).toHaveLength(1);
      fireEvent.click(exportButtons[0]);
      expect(exportBugReportMock).toHaveBeenCalledWith("req-captured");
    });
  });

  describe("request trace inspector (CCS-139)", () => {
    const row = (requestId: string, dataSource: string) => ({
      requestId,
      providerId: "b",
      providerName: "Provider B",
      appType: "claude",
      model: "claude-sonnet-4-5",
      requestModel: "claude-sonnet-4-5",
      costMultiplier: "1",
      inputTokens: 10,
      outputTokens: 2,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      inputCostUsd: "0",
      outputCostUsd: "0",
      cacheReadCostUsd: "0",
      cacheCreationCostUsd: "0",
      totalCostUsd: "0.0001",
      isStreaming: false,
      latencyMs: 60,
      firstTokenMs: null,
      statusCode: 200,
      createdAt: 1_710_000_000,
      dataSource,
    });

    it("opens the trace panel for proxied rows only", () => {
      useRequestLogsMock.mockReturnValue({
        data: {
          data: [row("req-proxy", "proxy"), row("req-session", "session")],
          total: 2,
          page: 0,
          pageSize: 20,
        },
        isLoading: false,
      });
      traceQueryMock.mockImplementation((requestId: string | null) => ({
        data:
          requestId === "req-proxy"
            ? {
                traceId: "t1",
                requestId: "req-proxy",
                startedAt: 0,
                appType: "claude",
                method: "POST",
                path: "/v1/messages",
                isStream: false,
                clientHeaders: [],
                attempts: [
                  {
                    providerId: "a",
                    providerName: "Provider A",
                    status: "failed",
                    reason: "上游错误 (503): overloaded",
                    upstreamStatus: 503,
                    plan: null,
                    recovery: [],
                    upstreamHeaders: [],
                    startedMs: 0,
                    durationMs: 5,
                  },
                  {
                    providerId: "b",
                    providerName: "Provider B",
                    status: "succeeded",
                    reason: null,
                    upstreamStatus: 200,
                    plan: null,
                    recovery: [],
                    upstreamHeaders: [],
                    startedMs: 6,
                    durationMs: 9,
                  },
                ],
                droppedAttempts: 0,
                statusCode: 200,
                error: null,
                ttfbMs: 15,
                firstTokenMs: null,
                latencyMs: 20,
                usageSource: "response_body",
              }
            : null,
        isLoading: false,
      }));
      render(
        <RequestLogTable
          range={{ preset: "today" }}
          rangeLabel="Today"
          appType="all"
          refreshIntervalMs={0}
        />,
      );

      const inspect = screen.getAllByRole("button", {
        name: "usage.trace.inspect",
      });
      expect(inspect).toHaveLength(1);
      expect(screen.queryByText("usage.trace.title")).toBeNull();

      fireEvent.click(inspect[0]);
      expect(traceQueryMock).toHaveBeenLastCalledWith("req-proxy");
      expect(screen.getByText("usage.trace.title")).toBeInTheDocument();
      expect(screen.getAllByTestId("trace-attempt")).toHaveLength(2);
      expect(
        screen.getByText("上游错误 (503): overloaded"),
      ).toBeInTheDocument();
    });
  });
});
