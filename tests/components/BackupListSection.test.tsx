import type { ReactElement } from "react";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

const stableT = vi.hoisted(
  () => (key: string, options?: { defaultValue?: string }) =>
    options?.defaultValue ?? key,
);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT }),
}));

const renderWithQuery = (ui: ReactElement) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const backups = [
  {
    filename: "db_backup_20260102_030405.db",
    sizeBytes: 2048,
    createdAt: "2026-01-02T03:04:05Z",
  },
  {
    filename: "before-upgrade.db",
    sizeBytes: 512,
    createdAt: "2026-01-01T00:00:00Z",
  },
];

beforeEach(() => {
  toastMock.error.mockReset();
  toastMock.success.mockReset();
});

describe("BackupListSection", () => {
  it("lists backups with parsed names and sizes", async () => {
    mockInvoke({ list_db_backups: backups });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);

    expect(await screen.findByText("2026-01-02 03:04:05")).toBeInTheDocument();
    expect(screen.getByText("before-upgrade")).toBeInTheDocument();
    expect(screen.getByText(/2\.0 KB/)).toBeInTheDocument();
    expect(screen.getByText(/512 B/)).toBeInTheDocument();
  });

  it("shows the empty state", async () => {
    mockInvoke({ list_db_backups: [] });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);
    expect(await screen.findByText("No backups yet")).toBeInTheDocument();
  });

  it("restores only after confirmation and reports the safety backup", async () => {
    const api = mockInvoke({
      list_db_backups: backups,
      restore_db_backup: "safety-123",
    });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);

    await screen.findByText("before-upgrade");
    fireEvent.click(screen.getAllByRole("button", { name: /Restore/ })[1]);
    expect(api.callsTo("restore_db_backup")).toHaveLength(0);

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Confirm Restore");
    const confirm = Array.from(dialog.querySelectorAll("button")).find(
      (b) => b.textContent === "Restore",
    )!;
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(api.callsTo("restore_db_backup")).toHaveLength(1),
    );
    expect(api.callsTo("restore_db_backup")[0].args).toEqual({
      filename: "before-upgrade.db",
    });
    await waitFor(() => expect(toastMock.success).toHaveBeenCalled());
    expect(toastMock.success.mock.calls[0][1]?.description).toContain(
      "safety-123",
    );
  });

  it("shows the backend error when a restore fails", async () => {
    mockInvoke({
      list_db_backups: backups,
      restore_db_backup: () => {
        throw new Error("backup failed integrity check");
      },
    });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);

    await screen.findByText("before-upgrade");
    fireEvent.click(screen.getAllByRole("button", { name: /Restore/ })[0]);
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      Array.from(dialog.querySelectorAll("button")).find(
        (b) => b.textContent === "Restore",
      )!,
    );

    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(String(toastMock.error.mock.calls[0][0])).toContain(
      "integrity check",
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("renames a backup on Enter with the IPC argument names the backend expects", async () => {
    const api = mockInvoke({
      list_db_backups: backups,
      rename_db_backup: "nightly.db",
    });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);

    await screen.findByText("before-upgrade");
    fireEvent.click(screen.getAllByTitle("Rename")[1]);
    const input = screen.getByPlaceholderText("Enter new name");
    expect(input).toHaveValue("before-upgrade");
    fireEvent.change(input, { target: { value: "  nightly  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(api.callsTo("rename_db_backup")).toHaveLength(1),
    );
    expect(api.callsTo("rename_db_backup")[0].args).toEqual({
      oldFilename: "before-upgrade.db",
      newName: "nightly",
    });
  });

  it("deletes a backup only after confirmation", async () => {
    const api = mockInvoke({
      list_db_backups: backups,
      delete_db_backup: null,
    });
    renderWithQuery(<BackupListSection onSettingsChange={vi.fn()} />);

    await screen.findByText("before-upgrade");
    fireEvent.click(screen.getAllByTitle("Delete")[0]);
    expect(api.callsTo("delete_db_backup")).toHaveLength(0);

    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      Array.from(dialog.querySelectorAll("button")).find(
        (b) => b.textContent === "Delete",
      )!,
    );
    await waitFor(() =>
      expect(api.callsTo("delete_db_backup")).toHaveLength(1),
    );
    expect(api.callsTo("delete_db_backup")[0].args).toEqual({
      filename: "db_backup_20260102_030405.db",
    });
  });
});
