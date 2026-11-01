import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { RequestLog } from "@/types/usage";
import {
  csvField,
  defaultExportFileName,
  fetchAllRequestLogs,
  logsToCsv,
  USAGE_CSV_COLUMNS,
} from "@/components/usage/usageExport";
import { UsageExportButton } from "@/components/usage/UsageExportButton";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

const log = (i: number, overrides: Partial<RequestLog> = {}): RequestLog => ({
  requestId: `r-${i}`,
  providerId: "p",
  providerName: "Relay",
  appType: "claude",
  model: "claude-sonnet-4",
  costMultiplier: "1",
  inputTokens: 10,
  outputTokens: 5,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  inputCostUsd: "0.1",
  outputCostUsd: "0.2",
  cacheReadCostUsd: "0",
  cacheCreationCostUsd: "0",
  totalCostUsd: "0.3",
  isStreaming: true,
  latencyMs: 100,
  statusCode: 200,
  createdAt: 1_700_000_000 + i,
  ...overrides,
});

describe("usage export (FE-UI-25 / PRODUCT-50)", () => {
  it("quotes CSV fields and neutralises formulas", () => {
    expect(csvField('say "hi", ok')).toBe('"say ""hi"", ok"');
    expect(csvField("line\nbreak")).toBe('"line\nbreak"');
    expect(csvField("=HYPERLINK(1)")).toBe("'=HYPERLINK(1)");
    expect(csvField(-5)).toBe("-5");
    expect(csvField(undefined)).toBe("");
  });

  it("writes one header and one line per log", () => {
    const csv = logsToCsv([log(1), log(2, { errorMessage: "bad, request" })]);
    const lines = csv.trimEnd().split("\r\n");
    expect(lines).toHaveLength(3);
    expect(lines[0].split(",")).toEqual(USAGE_CSV_COLUMNS.map((c) => c.header));
    expect(lines[2]).toContain('"bad, request"');
  });

  it("pages through every row of the filter", async () => {
    const all = Array.from({ length: 1203 }, (_, i) => log(i));
    const api = mockInvoke({
      get_request_logs: ({ page, pageSize }: Record<string, unknown>) => {
        const start = (page as number) * (pageSize as number);
        return {
          data: all.slice(start, start + (pageSize as number)),
          total: all.length,
          page,
          pageSize,
        };
      },
    });
    const { logs, total } = await fetchAllRequestLogs({ appType: "claude" });
    expect(total).toBe(1203);
    expect(logs).toHaveLength(1203);
    expect(api.callsTo("get_request_logs")).toHaveLength(3);
    expect(api.callsTo("get_request_logs")[0].args).toMatchObject({
      filters: { appType: "claude" },
    });
  });

  it("names files with the format extension", () => {
    expect(defaultExportFileName("csv", new Date(2026, 0, 2, 3, 4))).toBe(
      "cc-switch-usage-20260102-0304.csv",
    );
  });
});

describe("UsageExportButton", () => {
  beforeEach(() => {
    toastMock.success.mockReset();
    toastMock.error.mockReset();
  });

  const openAndPick = async (label: RegExp) => {
    const trigger = screen.getByRole("button", { name: /Export/ });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: label }));
  };

  it("exports the active filter as CSV through the save command", async () => {
    const api = mockInvoke({
      get_request_logs: {
        data: [log(1), log(2)],
        total: 2,
        page: 0,
        pageSize: 500,
      },
      save_usage_export: "/tmp/usage.csv",
    });
    render(
      <UsageExportButton
        range={{ preset: "7d" }}
        appType="codex"
        providerName="Relay"
      />,
    );
    await openAndPick(/as CSV/);

    await waitFor(() =>
      expect(api.callsTo("save_usage_export")).toHaveLength(1),
    );
    const args = api.callsTo("save_usage_export")[0].args as {
      format: string;
      content: string;
      defaultName: string;
    };
    expect(args.format).toBe("csv");
    expect(args.defaultName).toMatch(/\.csv$/);
    expect(args.content.trimEnd().split("\r\n")).toHaveLength(3);
    expect(api.callsTo("get_request_logs")[0].args).toMatchObject({
      filters: { appType: "codex", providerName: "Relay" },
    });
    await waitFor(() => expect(toastMock.success).toHaveBeenCalled());
  });

  it("stays silent when the save dialog is cancelled", async () => {
    const api = mockInvoke({
      get_request_logs: { data: [], total: 0, page: 0, pageSize: 500 },
      save_usage_export: null,
    });
    render(<UsageExportButton range={{ preset: "today" }} />);
    await openAndPick(/as JSON/);

    await waitFor(() =>
      expect(api.callsTo("save_usage_export")).toHaveLength(1),
    );
    expect(toastMock.success).not.toHaveBeenCalled();
    expect(toastMock.error).not.toHaveBeenCalled();
  });
});
