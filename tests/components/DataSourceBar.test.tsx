import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DataSourceBar } from "@/components/usage/DataSourceBar";
import { mockInvoke } from "../msw/invokeRecorder";

// TEST-12: IPC-level mocks; the real usageApi wrapper runs.
const toast = vi.hoisted(() => ({
  success: vi.fn(),
  info: vi.fn(),
  error: vi.fn(),
}));

vi.mock("sonner", () => ({ toast }));
const renderBar = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <DataSourceBar refreshIntervalMs={0} />
    </QueryClientProvider>,
  );
  return { invalidate };
};

describe("DataSourceBar", () => {
  beforeEach(() => {
    Object.values(toast).forEach((fn) => fn.mockReset());
  });

  it("lists data sources and imports session logs on demand", async () => {
    const api = mockInvoke({
      get_usage_data_sources: [
        { dataSource: "proxy", requestCount: 12, totalCostUsd: "1" },
        { dataSource: "session_log", requestCount: 3, totalCostUsd: "0.5" },
      ],
      sync_session_usage: {
        imported: 4,
        skipped: 0,
        filesScanned: 2,
        errors: [],
      },
    });
    const { invalidate } = renderBar();

    await waitFor(() =>
      expect(screen.getByText("Routing")).toBeInTheDocument(),
    );
    expect(screen.getByText("Session Log")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Sync/ }));
    await waitFor(() =>
      expect(api.callsTo("sync_session_usage")).toHaveLength(1),
    );
    await waitFor(() => expect(toast.success).toHaveBeenCalled());
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["usage"] });
  });

  it("renders nothing when there is no usage data yet", async () => {
    const api = mockInvoke({ get_usage_data_sources: [] });
    renderBar();
    await waitFor(() =>
      expect(api.callsTo("get_usage_data_sources")).toHaveLength(1),
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
