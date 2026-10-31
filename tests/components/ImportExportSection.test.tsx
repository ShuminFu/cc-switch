import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { within } from "@testing-library/react";
import { ImportExportSection } from "@/components/settings/ImportExportSection";

const tMock = vi.fn((key: string) => key);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

describe("ImportExportSection Component", () => {
  const baseProps = {
    status: "idle" as const,
    selectedFile: "",
    errorMessage: null,
    backupId: null,
    isImporting: false,
    onSelectFile: vi.fn(),
    onImport: vi.fn(),
    onExport: vi.fn(),
    onClear: vi.fn(),
  };

  beforeEach(() => {
    tMock.mockImplementation((key: string) => key);
    baseProps.onSelectFile.mockReset();
    baseProps.onImport.mockReset();
    baseProps.onExport.mockReset();
    baseProps.onClear.mockReset();
  });

  it("should disable import button and show placeholder when no file selected", () => {
    render(<ImportExportSection {...baseProps} />);

    // When no file selected, button shows "selectConfigFile" and clicking it opens file dialog
    expect(
      screen.getByRole("button", { name: /settings\.selectConfigFile/ }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.exportConfig" }),
    );
    expect(baseProps.onExport).toHaveBeenCalledTimes(1);

    fireEvent.click(
      screen.getByRole("button", { name: /settings\.selectConfigFile/ }),
    );
    expect(baseProps.onSelectFile).toHaveBeenCalledTimes(1);
  });

  it("should show filename and enable import/clear when file is selected", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
      />,
    );

    expect(screen.getByText(/config\.json/)).toBeInTheDocument();
    const importButton = screen.getByRole("button", {
      name: /settings\.import/,
    });
    expect(importButton).toBeEnabled();
    fireEvent.click(importButton);
    // FE-UI-55: importing replaces everything, so it asks first.
    expect(baseProps.onImport).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "settings.import" }),
    );
    expect(baseProps.onImport).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    expect(baseProps.onClear).toHaveBeenCalledTimes(1);
  });

  it("should show loading text and disable import button during import", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
        isImporting
        status="importing"
      />,
    );

    const importingLabels = screen.getAllByText("settings.importing");
    expect(importingLabels.length).toBeGreaterThanOrEqual(2);
    expect(
      screen.getByRole("button", { name: "settings.importing" }),
    ).toBeDisabled();
    expect(screen.getByText("common.loading")).toBeInTheDocument();
  });

  it("should display backup information on successful import", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
        status="success"
        backupId="backup-001"
      />,
    );

    expect(screen.getByText("settings.importSuccess")).toBeInTheDocument();
    expect(screen.getByText(/backup-001/)).toBeInTheDocument();
    expect(screen.getByText("settings.autoReload")).toBeInTheDocument();
  });

  it("should display error message when import fails", () => {
    render(
      <ImportExportSection
        {...baseProps}
        status="error"
        errorMessage="Parse failed"
      />,
    );

    expect(screen.getByText("settings.importFailed")).toBeInTheDocument();
    expect(screen.getByText("Parse failed")).toBeInTheDocument();
  });

  it("does not import when the confirmation is cancelled", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.sql"}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /settings\.import/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("settings.importConfirmMessage");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(baseProps.onImport).not.toHaveBeenCalled();
  });

  it("offers a revert that restores the pre-import backup", async () => {
    const onRevertImport = vi.fn().mockResolvedValue(undefined);
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.sql"}
        status="success"
        backupId="db_safety_20260101_000000"
        onRevertImport={onRevertImport}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.revertImport" }),
    );
    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "settings.revertImport" }),
    );
    await waitFor(() =>
      expect(onRevertImport).toHaveBeenCalledWith("db_safety_20260101_000000"),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "settings.revertImport" }),
      ).toBeNull(),
    );
  });
});
