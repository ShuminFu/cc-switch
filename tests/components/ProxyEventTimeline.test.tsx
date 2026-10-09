import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProxyEventTimeline,
  formatEventTime,
} from "@/components/proxy/ProxyEventTimeline";
import type { ProxyEvent } from "@/types/proxy";

const apiMock = vi.hoisted(() => ({
  getProxyEvents: vi.fn(),
  clearProxyEvents: vi.fn(),
}));
vi.mock("@/lib/api/proxy", () => ({ proxyApi: apiMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      (options?.defaultValue as string | undefined) ?? key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

const events: ProxyEvent[] = [
  {
    id: 3,
    createdAt: 1_800_000_300,
    appType: "claude",
    kind: "failover_switch",
    providerId: "p2",
    providerName: "Backup Relay",
  },
  {
    id: 2,
    createdAt: 1_800_000_200,
    appType: "codex",
    kind: "breaker_open",
    providerId: "c1",
    detail: "5 consecutive failures",
  },
  { id: 1, createdAt: 1_800_000_100, appType: "*", kind: "proxy_start" },
];

const renderTimeline = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ProxyEventTimeline />
    </QueryClientProvider>,
  );
};

describe("ProxyEventTimeline", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.getProxyEvents.mockResolvedValue(events);
    apiMock.clearProxyEvents.mockResolvedValue(3);
  });

  it("formats event times in the given locale", () => {
    expect(formatEventTime(1_800_000_000, "en")).toMatch(/\d/);
  });

  it("lists events newest first with kind, app, provider and detail", async () => {
    renderTimeline();
    const first = await screen.findByTestId("proxy-event-3");
    expect(first).toHaveTextContent("故障转移切换");
    expect(first).toHaveTextContent("claude");
    expect(first).toHaveTextContent("Backup Relay");

    const second = screen.getByTestId("proxy-event-2");
    expect(second).toHaveTextContent("熔断器打开");
    expect(second).toHaveTextContent("c1");
    expect(second).toHaveTextContent("5 consecutive failures");

    // global events carry no app chip
    expect(screen.getByTestId("proxy-event-1")).not.toHaveTextContent("*");
    expect(apiMock.getProxyEvents).toHaveBeenCalledWith(undefined, 50);
  });

  it("filters by app and clears the filtered scope", async () => {
    renderTimeline();
    await screen.findByTestId("proxy-event-3");

    fireEvent.click(screen.getByRole("button", { name: "Codex" }));
    await waitFor(() =>
      expect(apiMock.getProxyEvents).toHaveBeenCalledWith("codex", 50),
    );

    const clearButton = screen.getByRole("button", { name: /清空/ });
    await waitFor(() => expect(clearButton).not.toBeDisabled());
    fireEvent.click(clearButton);
    await waitFor(() =>
      expect(apiMock.clearProxyEvents).toHaveBeenCalledWith("codex"),
    );
  });

  it("shows the empty hint when nothing was recorded", async () => {
    apiMock.getProxyEvents.mockResolvedValue([]);
    renderTimeline();
    expect(await screen.findByText(/还没有事件/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /清空/ })).toBeDisabled();
  });
});
