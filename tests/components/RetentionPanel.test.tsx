import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RetentionPanel } from "@/components/settings/RetentionPanel";
import type { DbStorageStats, MaintenanceReport } from "@/shared/bindings";
import { command } from "../msw/handlers";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

// 返回 key，并把插值参数附在后面，便于断言传入的值
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "en" },
  }),
}));

const statsBefore: DbStorageStats = {
  sizeBytes: 3 * 1024 * 1024,
  freeBytes: 512 * 1024,
  tables: [
    { name: "proxy_request_logs", rows: 12345 },
    { name: "usage_daily_rollups", rows: 40 },
  ],
};

const statsAfter: DbStorageStats = {
  sizeBytes: 2 * 1024 * 1024,
  freeBytes: 0,
  tables: [
    { name: "proxy_request_logs", rows: 100 },
    { name: "usage_daily_rollups", rows: 52 },
  ],
};

const report: MaintenanceReport = {
  streamCheckLogsDeleted: 3,
  detailRowsRolledUp: 12245,
  rollupRowsDeleted: 0,
  backupsDeleted: 1,
};

const renderPanel = (
  props: Partial<React.ComponentProps<typeof RetentionPanel>> = {},
) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onSettingsChange = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <RetentionPanel onSettingsChange={onSettingsChange} {...props} />
    </QueryClientProvider>,
  );
  return { onSettingsChange };
};

const rowCount = (table: string) => {
  const row = screen.getByText(table).closest("tr");
  if (!row) throw new Error(`row for ${table} not found`);
  return within(row).getAllByRole("cell")[1].textContent;
};

describe("RetentionPanel", () => {
  let maintenanceCalls: number;

  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    maintenanceCalls = 0;
    server.use(
      command("get_db_storage_stats", () =>
        maintenanceCalls > 0 ? statsAfter : statsBefore,
      ),
      command("run_db_maintenance", () => {
        maintenanceCalls += 1;
        return report;
      }),
    );
  });

  it("shows the database size and row counts per table", async () => {
    renderPanel();

    expect(await screen.findByText("proxy_request_logs")).toBeInTheDocument();
    expect(rowCount("proxy_request_logs")).toBe("12,345");
    expect(rowCount("usage_daily_rollups")).toBe("40");
    expect(screen.getByTestId("db-size")).toHaveTextContent(
      'settings.advanced.retention.dbSize {"size":"3.0 MB","free":"512.0 KB"}',
    );
  });

  it("shows the default policy when the settings are unset", async () => {
    renderPanel();

    expect(
      screen.getByRole("combobox", {
        name: "settings.advanced.retention.detailDaysLabel",
      }),
    ).toHaveTextContent('settings.advanced.retention.days {"days":30}');
    expect(
      screen.getByRole("combobox", {
        name: "settings.advanced.retention.rollupDaysLabel",
      }),
    ).toHaveTextContent("settings.advanced.retention.keepForever");
  });

  it("saves new retention values through onSettingsChange", async () => {
    const user = userEvent.setup();
    const { onSettingsChange } = renderPanel({
      usageDetailRetentionDays: 45,
      usageRollupRetentionDays: 365,
    });

    const detail = screen.getByRole("combobox", {
      name: "settings.advanced.retention.detailDaysLabel",
    });
    // 手动写入的非预设值也能显示
    expect(detail).toHaveTextContent(
      'settings.advanced.retention.days {"days":45}',
    );
    await user.click(detail);
    await user.click(
      await screen.findByRole("option", {
        name: 'settings.advanced.retention.days {"days":90}',
      }),
    );
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      usageDetailRetentionDays: 90,
    });

    await user.click(
      screen.getByRole("combobox", {
        name: "settings.advanced.retention.rollupDaysLabel",
      }),
    );
    await user.click(
      await screen.findByRole("option", {
        name: "settings.advanced.retention.keepForever",
      }),
    );
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      usageRollupRetentionDays: 0,
    });
  });

  it("runs maintenance now, reports the result and refreshes the stats", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("proxy_request_logs");

    await user.click(
      screen.getByRole("button", {
        name: "settings.advanced.retention.runNow",
      }),
    );

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledTimes(1));
    expect(maintenanceCalls).toBe(1);
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.advanced.retention.maintenanceDone",
      {
        description:
          'settings.advanced.retention.maintenanceSummary {"detail":12245,"rollups":0,"checks":3,"backups":1}',
      },
    );
    await waitFor(() => expect(rowCount("proxy_request_logs")).toBe("100"));
    expect(rowCount("usage_daily_rollups")).toBe("52");
  });

  it("shows the backend error when maintenance fails", async () => {
    server.use(
      command("run_db_maintenance", () =>
        HttpResponse.json("database is locked", { status: 500 }),
      ),
    );
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("proxy_request_logs");

    await user.click(
      screen.getByRole("button", {
        name: "settings.advanced.retention.runNow",
      }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("database is locked"),
      ),
    );
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });
});
