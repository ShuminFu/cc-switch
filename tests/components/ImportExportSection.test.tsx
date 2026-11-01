import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { within } from "@testing-library/react";
import { ImportExportSection } from "@/components/settings/ImportExportSection";

const tMock = vi.fn((key: string) => key);

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
      screen.getByRole("button", { name: /Select SQL File/ }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Export SQL Backup" }));
    expect(baseProps.onExport).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: /Select SQL File/ }));
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
      name: /Import/,
    });
    expect(importButton).toBeEnabled();
    fireEvent.click(importButton);
    // FE-UI-55: importing replaces everything, so it asks first.
    expect(baseProps.onImport).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Import" }));
    expect(baseProps.onImport).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
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

    const importingLabels = screen.getAllByText("Importing...");
    expect(importingLabels.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByRole("button", { name: "Importing..." })).toBeDisabled();
    expect(screen.getByText("Loading...")).toBeInTheDocument();
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

    expect(screen.getByText("Import Successful!")).toBeInTheDocument();
    expect(screen.getByText(/backup-001/)).toBeInTheDocument();
    expect(screen.getByText("Data refreshed")).toBeInTheDocument();
  });

  it("should display error message when import fails", () => {
    render(
      <ImportExportSection
        {...baseProps}
        status="error"
        errorMessage="Parse failed"
      />,
    );

    expect(screen.getByText("Import Failed")).toBeInTheDocument();
    expect(screen.getByText("Parse failed")).toBeInTheDocument();
  });

  it("does not import when the confirmation is cancelled", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.sql"}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Import/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent(
      "Importing config.sql replaces all providers",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
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

    fireEvent.click(screen.getByRole("button", { name: "Revert import" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Revert import" }),
    );
    await waitFor(() =>
      expect(onRevertImport).toHaveBeenCalledWith("db_safety_20260101_000000"),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Revert import" }),
      ).toBeNull(),
    );
  });
});
