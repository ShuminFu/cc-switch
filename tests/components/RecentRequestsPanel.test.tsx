import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { RecentRequestsPanel } from "@/components/proxy/RecentRequestsPanel";
import type { ProxyStatus } from "@/types/proxy";
import type { RequestLog } from "@/types/usage";
import { mockInvoke } from "../msw/invokeRecorder";
import { createTestQueryClient } from "../utils/testQueryClient";

const status: ProxyStatus = {
  running: true,
  address: "127.0.0.1",
  port: 15721,
  active_connections: 0,
  total_requests: 2,
  success_requests: 1,
  failed_requests: 1,
  success_rate: 50,
  uptime_seconds: 60,
  current_provider: null,
  current_provider_id: null,
  last_request_at: "2026-10-31T08:00:00Z",
  last_error: "HTTP 401 invalid api key",
  failover_count: 0,
  active_targets: [],
} as ProxyStatus;

const log = (id: string, statusCode: number, extra: Partial<RequestLog> = {}) =>
  ({
    requestId: id,
    providerId: "p1",
    providerName: "Relay",
    appType: "claude",
    model: "claude-sonnet",
    costMultiplier: "1",
    inputTokens: 1,
    outputTokens: 1,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    inputCostUsd: "0",
    outputCostUsd: "0",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0",
    isStreaming: false,
    latencyMs: 120,
    statusCode,
    createdAt: 1_790_000_000,
    ...extra,
  }) as RequestLog;

const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <RecentRequestsPanel status={status} />
    </QueryClientProvider>,
  );

describe("RecentRequestsPanel (FE-UI-39)", () => {
  it("shows the last error, recent rows and opens the detail dialog", async () => {
    const api = mockInvoke({
      get_request_logs: {
        data: [
          log("r2", 401, { errorMessage: "invalid api key" }),
          log("r1", 200),
        ],
        total: 2,
        page: 0,
        pageSize: 10,
      },
      get_request_detail: log("r2", 401, { errorMessage: "invalid api key" }),
    });
    renderPanel();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "HTTP 401 invalid api key",
    );
    const failed = await screen.findByRole("button", {
      name: /Request 401 claude-sonnet/,
    });
    expect(failed).toHaveTextContent("invalid api key");
    expect(api.callsTo("get_request_logs")[0].args).toMatchObject({
      page: 0,
      pageSize: 10,
    });

    fireEvent.click(failed);
    await waitFor(() =>
      expect(api.callsTo("get_request_detail")[0]?.args).toMatchObject({
        requestId: "r2",
      }),
    );
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("filters to failed requests", async () => {
    const api = mockInvoke({
      get_request_logs: { data: [], total: 0, page: 0, pageSize: 10 },
    });
    renderPanel();
    await screen.findByText("No requests today");
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(
        api
          .callsTo("get_request_logs")
          .some(
            (c) =>
              (c.args.filters as Record<string, unknown>)?.statusMin === 400,
          ),
      ).toBe(true),
    );
  });
});
