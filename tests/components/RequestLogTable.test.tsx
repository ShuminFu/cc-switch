import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { RequestLog, UsageRangeSelection } from "@/types/usage";

const useRequestLogsMock = vi.hoisted(() => vi.fn());
const usageApiMock = vi.hoisted(() => ({
  getUnpricedModelStats: vi.fn(),
  saveCsvFileDialog: vi.fn(),
  writeCsvExport: vi.fn(),
}));
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: {
        defaultValue?: string;
      },
    ) => options?.defaultValue ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));

vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("@/components/usage/PricingEditModal", () => ({
  PricingEditModal: ({ model, isNew }: any) => (
    <div data-testid="pricing-modal" data-new={String(!!isNew)}>
      {model.modelId}
    </div>
  ),
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

function makeLog(overrides: Partial<RequestLog>): RequestLog {
  return {
    requestId: "req",
    providerId: "p1",
    providerName: "Provider",
    appType: "claude",
    model: "model",
    costMultiplier: "1",
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    inputCostUsd: "0",
    outputCostUsd: "0",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0",
    isStreaming: false,
    latencyMs: 1000,
    firstTokenMs: null,
    durationMs: null,
    statusCode: 200,
    errorMessage: null,
    createdAt: 1_710_000_000,
    dataSource: "proxy",
    ...overrides,
  };
}

const pricedLog = makeLog({
  requestId: "priced",
  model: "priced-model",
  totalCostUsd: "0.0123",
});
const unpricedLog = makeLog({
  requestId: "unpriced",
  model: "upstream-echo",
  pricingModel: "mystery-model",
});

describe("RequestLogTable", () => {
  beforeEach(() => {
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

  describe("unpriced requests", () => {
    beforeEach(() => {
      usageApiMock.getUnpricedModelStats.mockReset();
      usageApiMock.saveCsvFileDialog.mockReset();
      usageApiMock.writeCsvExport.mockReset();
      toastMock.success.mockReset();
      toastMock.info.mockReset();
      toastMock.error.mockReset();
      // 与后端同口径：带 unpriced 筛选时只返回未计价行
      useRequestLogsMock.mockImplementation(
        ({ filters }: { filters: { unpriced?: boolean } }) => {
          const data = filters.unpriced
            ? [unpricedLog]
            : [pricedLog, unpricedLog];
          return {
            data: { data, total: data.length, page: 0, pageSize: 20 },
            isLoading: false,
          };
        },
      );
    });

    it("filter shows only unpriced rows", async () => {
      render(
        <RequestLogTable
          range={{ preset: "today" }}
          rangeLabel="Today"
          appType="all"
          refreshIntervalMs={0}
        />,
      );

      expect(screen.getByText("priced-model")).toBeInTheDocument();
      expect(screen.getByText("upstream-echo")).toBeInTheDocument();

      const toggle = screen.getByRole("button", { name: "usage.unpricedOnly" });
      expect(toggle).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(toggle);

      await waitFor(() => {
        expect(useRequestLogsMock).toHaveBeenLastCalledWith(
          expect.objectContaining({
            page: 0,
            filters: expect.objectContaining({ unpriced: true }),
          }),
        );
      });
      expect(toggle).toHaveAttribute("aria-pressed", "true");
      expect(screen.queryByText("priced-model")).not.toBeInTheDocument();
      expect(screen.getByText("upstream-echo")).toBeInTheDocument();
      expect(screen.getAllByText("usage.unpriced")).toHaveLength(1);

      fireEvent.click(toggle);
      await waitFor(() => {
        expect(useRequestLogsMock).toHaveBeenLastCalledWith(
          expect.objectContaining({
            filters: expect.objectContaining({ unpriced: undefined }),
          }),
        );
      });
      expect(screen.getByText("priced-model")).toBeInTheDocument();
    });

    it("exports unpriced requests grouped by model as CSV", async () => {
      usageApiMock.getUnpricedModelStats.mockResolvedValue([
        {
          model: "mystery-model",
          requestCount: 3,
          inputTokens: 300,
          outputTokens: 150,
          cacheReadTokens: 10,
          cacheCreationTokens: 0,
          firstSeenAt: 1_710_000_000,
          lastSeenAt: 1_710_000_600,
        },
        {
          model: "other,model",
          requestCount: 1,
          inputTokens: 5,
          outputTokens: 1,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          firstSeenAt: 1_710_000_100,
          lastSeenAt: 1_710_000_100,
        },
      ]);
      usageApiMock.saveCsvFileDialog.mockResolvedValue("/tmp/unpriced.csv");
      usageApiMock.writeCsvExport.mockResolvedValue(undefined);

      render(
        <RequestLogTable
          range={{
            preset: "custom",
            customStartDate: 1_710_000_000,
            customEndDate: 1_710_086_400,
          }}
          rangeLabel="Custom"
          appType="claude"
          model="mystery-model"
          refreshIntervalMs={0}
        />,
      );

      fireEvent.click(
        screen.getByRole("button", { name: "usage.exportUnpriced" }),
      );

      await waitFor(() => {
        expect(usageApiMock.writeCsvExport).toHaveBeenCalledTimes(1);
      });
      expect(usageApiMock.getUnpricedModelStats).toHaveBeenCalledWith(
        expect.objectContaining({
          appType: "claude",
          model: "mystery-model",
          unpriced: true,
          startDate: 1_710_000_000,
          endDate: 1_710_086_400,
        }),
      );
      expect(usageApiMock.saveCsvFileDialog).toHaveBeenCalledWith(
        expect.stringMatching(/^unpriced-models-.*\.csv$/),
      );

      const [path, csv] = usageApiMock.writeCsvExport.mock.calls[0];
      expect(path).toBe("/tmp/unpriced.csv");
      const lines = csv.trimEnd().split("\r\n");
      expect(lines[0]).toBe(
        "model,request_count,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,first_seen_at,last_seen_at",
      );
      expect(lines[1]).toMatch(/^mystery-model,3,300,150,10,0,/);
      expect(lines[2]).toMatch(/^"other,model",1,5,1,0,0,/);
      expect(toastMock.success).toHaveBeenCalled();
    });

    it("does not open a save dialog when nothing is unpriced", async () => {
      usageApiMock.getUnpricedModelStats.mockResolvedValue([]);

      render(
        <RequestLogTable
          range={{ preset: "today" }}
          rangeLabel="Today"
          refreshIntervalMs={0}
        />,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "usage.exportUnpriced" }),
      );

      await waitFor(() => {
        expect(toastMock.info).toHaveBeenCalledWith(
          "usage.exportUnpricedEmpty",
        );
      });
      expect(usageApiMock.saveCsvFileDialog).not.toHaveBeenCalled();
      expect(usageApiMock.writeCsvExport).not.toHaveBeenCalled();
    });

    it("add pricing pre-fills the model id of the unpriced row", () => {
      render(
        <RequestLogTable
          range={{ preset: "today" }}
          rangeLabel="Today"
          refreshIntervalMs={0}
        />,
      );

      expect(screen.queryByTestId("pricing-modal")).not.toBeInTheDocument();
      // 只有未计价行提供入口；计价模型优先于上游回显的 model
      const addButtons = screen.getAllByRole("button", {
        name: /^usage\.addPricing: /,
      });
      expect(addButtons).toHaveLength(1);
      fireEvent.click(addButtons[0]);

      const modal = screen.getByTestId("pricing-modal");
      expect(modal).toHaveTextContent("mystery-model");
      expect(modal).toHaveAttribute("data-new", "true");
    });
  });
});
