import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  UsageExportMenu,
  buildUsageExportFileName,
} from "@/components/usage/UsageExportMenu";

const saveDialogMock = vi.hoisted(() => vi.fn());
const exportMock = vi.hoisted(() => vi.fn());
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    saveUsageExportDialog: saveDialogMock,
    exportUsageData: exportMock,
  },
}));

vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
  }),
}));

// Radix 下拉菜单依赖指针事件，这里用透传组件替代，直接渲染菜单项
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children }: any) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: any) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: any) => <div>{children}</div>,
  DropdownMenuLabel: ({ children }: any) => <div>{children}</div>,
  DropdownMenuSeparator: () => null,
  DropdownMenuItem: ({ children, onSelect }: any) => (
    <button type="button" onClick={() => onSelect?.()}>
      {children}
    </button>
  ),
}));

const filters = {
  appType: "claude",
  providerName: "Anthropic",
  startDate: 1_700_000_000,
  endDate: 1_700_086_400,
};

describe("buildUsageExportFileName", () => {
  it("stamps the kind, local date and extension", () => {
    expect(buildUsageExportFileName("logs", "csv", new Date(2026, 2, 5))).toBe(
      "cc-switch-usage-logs-20260305.csv",
    );
    expect(
      buildUsageExportFileName("statement", "json", new Date(2026, 11, 25)),
    ).toBe("cc-switch-usage-statement-20261225.json");
  });
});

describe("UsageExportMenu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("asks for a target path then exports with the current filters", async () => {
    saveDialogMock.mockResolvedValue("/exports/out.csv");
    exportMock.mockResolvedValue({
      path: "/exports/out.csv",
      rows: 12,
      truncated: false,
    });

    render(<UsageExportMenu filters={filters} />);
    fireEvent.click(screen.getByRole("button", { name: "请求日志 (CSV)" }));

    await waitFor(() =>
      expect(exportMock).toHaveBeenCalledWith({
        kind: "logs",
        format: "csv",
        filters,
        targetPath: "/exports/out.csv",
      }),
    );
    expect(saveDialogMock).toHaveBeenCalledWith(
      expect.stringMatching(/^cc-switch-usage-logs-\d{8}\.csv$/),
      "csv",
    );
    expect(toastMock.success).toHaveBeenCalledWith(
      "已导出 12 行到 /exports/out.csv",
    );
    expect(toastMock.warning).not.toHaveBeenCalled();
  });

  it("exports the statement as JSON and warns when rows were truncated", async () => {
    saveDialogMock.mockResolvedValue("/exports/statement.json");
    exportMock.mockResolvedValue({
      path: "/exports/statement.json",
      rows: 200000,
      truncated: true,
    });

    render(<UsageExportMenu filters={filters} />);
    fireEvent.click(screen.getByRole("button", { name: "账单汇总 (JSON)" }));

    await waitFor(() =>
      expect(exportMock).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "statement", format: "json" }),
      ),
    );
    expect(saveDialogMock).toHaveBeenCalledWith(
      expect.stringMatching(/^cc-switch-usage-statement-\d{8}\.json$/),
      "json",
    );
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    expect(String(toastMock.warning.mock.calls[0][0])).toContain("200000");
  });

  it("does nothing when the save dialog is cancelled", async () => {
    saveDialogMock.mockResolvedValue(null);

    render(<UsageExportMenu filters={filters} />);
    fireEvent.click(screen.getByRole("button", { name: "请求日志 (JSON)" }));

    await waitFor(() => expect(saveDialogMock).toHaveBeenCalledTimes(1));
    expect(exportMock).not.toHaveBeenCalled();
    expect(toastMock.success).not.toHaveBeenCalled();
    expect(toastMock.error).not.toHaveBeenCalled();
  });

  it("surfaces backend failures as an error toast", async () => {
    saveDialogMock.mockResolvedValue("/exports/out.csv");
    exportMock.mockRejectedValue(new Error("disk full"));

    render(<UsageExportMenu filters={filters} />);
    fireEvent.click(screen.getByRole("button", { name: "账单汇总 (CSV)" }));

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("disk full"),
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });
});
