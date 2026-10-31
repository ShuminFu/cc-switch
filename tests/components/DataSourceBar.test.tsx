import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DataSourceBar } from "@/components/usage/DataSourceBar";

const usageApiMock = vi.hoisted(() => ({
  getDataSourceBreakdown: vi.fn(),
  syncSessionUsage: vi.fn(),
}));
const toast = vi.hoisted(() => ({
  success: vi.fn(),
  info: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));
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
    Object.values(usageApiMock).forEach((fn) => fn.mockReset());
    Object.values(toast).forEach((fn) => fn.mockReset());
  });

  it("lists data sources and imports session logs on demand", async () => {
    usageApiMock.getDataSourceBreakdown.mockResolvedValue([
      { dataSource: "proxy", requestCount: 12, totalCostUsd: "1" },
      { dataSource: "session_log", requestCount: 3, totalCostUsd: "0.5" },
    ]);
    usageApiMock.syncSessionUsage.mockResolvedValue({
      imported: 4,
      skipped: 0,
      filesScanned: 2,
      errors: [],
    });
    const { invalidate } = renderBar();

    await waitFor(() =>
      expect(screen.getByText("Routing")).toBeInTheDocument(),
    );
    expect(screen.getByText("Session Log")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Sync/ }));
    await waitFor(() =>
      expect(usageApiMock.syncSessionUsage).toHaveBeenCalled(),
    );
    await waitFor(() => expect(toast.success).toHaveBeenCalled());
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["usage"] });
  });

  it("renders nothing when there is no usage data yet", async () => {
    usageApiMock.getDataSourceBreakdown.mockResolvedValue([]);
    renderBar();
    await waitFor(() =>
      expect(usageApiMock.getDataSourceBreakdown).toHaveBeenCalled(),
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
