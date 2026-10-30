import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DataSourceBar } from "@/components/usage/DataSourceBar";
import type { DataSourceSummary } from "@/types/usage";

const usageApiMock = vi.hoisted(() => ({
  getDataSourceBreakdown: vi.fn(),
  syncSessionUsage: vi.fn(),
  rescanSessionUsage: vi.fn(),
}));
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && "count" in options ? `${key}:${options.count}` : key,
  }),
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

function source(overrides: Partial<DataSourceSummary>): DataSourceSummary {
  return {
    dataSource: "session_log",
    requestCount: 3,
    totalCostUsd: "0.100000",
    duplicateCount: 0,
    unpricedCount: 0,
    missingSessionCount: 0,
    zeroOutputCount: 0,
    firstCreatedAt: 1_000,
    lastCreatedAt: 2_000,
    ...overrides,
  };
}

function renderBar() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DataSourceBar refreshIntervalMs={0} />
    </QueryClientProvider>,
  );
}

describe("DataSourceBar (CCS-171)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the per-source data-quality report and flags issues", async () => {
    usageApiMock.getDataSourceBreakdown.mockResolvedValue([
      source({ dataSource: "proxy", requestCount: 5 }),
      source({ unpricedCount: 2, zeroOutputCount: 1, duplicateCount: 4 }),
    ]);
    renderBar();

    const flagged = await screen.findByText("usage.dataSource.session_log");
    const chip = flagged.parentElement as HTMLElement;
    expect(chip.title).toContain("usage.dataQuality.duplicates:4");
    expect(chip.title).toContain("usage.dataQuality.unpriced:2");
    expect(chip.title).toContain("usage.dataQuality.zeroOutput:1");
    expect(screen.getAllByLabelText("usage.dataQuality.title")).toHaveLength(1);
  });

  it("runs a re-scan and reports how many rows were imported or repaired", async () => {
    usageApiMock.getDataSourceBreakdown.mockResolvedValue([source({})]);
    usageApiMock.rescanSessionUsage.mockResolvedValue({
      imported: 7,
      skipped: 0,
      filesScanned: 2,
      errors: [],
    });
    renderBar();

    fireEvent.click(await screen.findByTitle("usage.sessionSync.rescanTitle"));

    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith(
        "usage.sessionSync.rescanDone:7",
      ),
    );
    expect(usageApiMock.rescanSessionUsage).toHaveBeenCalledTimes(1);
    expect(usageApiMock.syncSessionUsage).not.toHaveBeenCalled();
  });
});
