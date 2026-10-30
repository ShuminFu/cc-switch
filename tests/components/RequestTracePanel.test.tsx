import type { ReactNode } from "react";
import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RequestTracePanel } from "@/components/usage/RequestTracePanel";
import type { RequestTrace } from "@/types/proxy";

const traceQueryMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) =>
      options?.count != null ? `${key}(${options.count})` : key,
  }),
}));

vi.mock("@/lib/query/proxy", () => ({
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

const plan = (url: string) => ({
  client: "AnthropicMessages",
  upstream: "AnthropicMessages",
  apiFormat: "anthropic",
  decode: "Passthrough",
  url,
  auth: "Anthropic",
  quirks: [],
  sideStores: [],
});

/** The trace the backend reports for a request that failed over from a to b. */
const failoverTrace: RequestTrace = {
  traceId: "trace-1",
  requestId: "req-1",
  startedAt: 1_710_000_000_000,
  appType: "claude",
  method: "POST",
  path: "/v1/messages",
  isStream: false,
  clientHeaders: [
    { name: "x-api-key", value: "[REDACTED]" },
    { name: "anthropic-version", value: "2023-06-01" },
  ],
  attempts: [
    {
      providerId: "a",
      providerName: "Provider A",
      status: "failed",
      reason: "上游错误 (503): overloaded",
      upstreamStatus: 503,
      plan: plan("https://a.example.com/v1/messages"),
      recovery: [],
      upstreamHeaders: [{ name: "x-api-key", value: "[REDACTED]" }],
      startedMs: 1,
      durationMs: 12,
    },
    {
      providerId: "b",
      providerName: "Provider B",
      status: "succeeded",
      reason: null,
      upstreamStatus: 200,
      plan: plan("https://b.example.com/v1/messages"),
      recovery: ["thinking_signature_rectifier"],
      upstreamHeaders: [],
      startedMs: 14,
      durationMs: 30,
    },
  ],
  droppedAttempts: 0,
  statusCode: 200,
  error: null,
  ttfbMs: 44,
  firstTokenMs: null,
  latencyMs: 60,
  usageSource: "response_body",
};

describe("RequestTracePanel", () => {
  beforeEach(() => {
    traceQueryMock.mockReset();
  });

  it("shows both attempts of a failover request with their reasons", () => {
    traceQueryMock.mockReturnValue({ data: failoverTrace, isLoading: false });
    render(<RequestTracePanel requestId="req-1" onClose={() => {}} />);

    expect(traceQueryMock).toHaveBeenCalledWith("req-1");
    expect(screen.getByText("usage.trace.attempts(2)")).toBeInTheDocument();
    const attempts = screen.getAllByTestId("trace-attempt");
    expect(attempts).toHaveLength(2);

    const [failed, succeeded] = attempts;
    expect(failed).toHaveAttribute("data-status", "failed");
    expect(within(failed).getByText("Provider A")).toBeInTheDocument();
    expect(within(failed).getByText("HTTP 503")).toBeInTheDocument();
    expect(
      within(failed).getByText("上游错误 (503): overloaded"),
    ).toBeInTheDocument();
    expect(
      within(failed).getByText("https://a.example.com/v1/messages"),
    ).toBeInTheDocument();

    expect(succeeded).toHaveAttribute("data-status", "succeeded");
    expect(within(succeeded).getByText("Provider B")).toBeInTheDocument();
    expect(
      within(succeeded).getByText("usage.trace.status.succeeded"),
    ).toBeInTheDocument();
    expect(
      within(succeeded).getByText("thinking_signature_rectifier"),
    ).toBeInTheDocument();
    expect(within(succeeded).queryByText(/usage.trace.reason/)).toBeNull();

    expect(screen.getByText("44ms")).toBeInTheDocument();
    expect(
      screen.getByText("usage.trace.usageSourceValue.response_body"),
    ).toBeInTheDocument();
    expect(screen.getByText("2023-06-01")).toBeInTheDocument();
  });

  it("explains that only recent requests are traced", () => {
    traceQueryMock.mockReturnValue({ data: null, isLoading: false });
    render(<RequestTracePanel requestId="req-old" onClose={() => {}} />);
    expect(screen.getByText("usage.trace.notFound(100)")).toBeInTheDocument();
    expect(screen.queryAllByTestId("trace-attempt")).toHaveLength(0);
  });
});
