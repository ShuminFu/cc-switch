import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { CodexHistoryBackupsRow } from "@/components/settings/CodexHistoryBackupsRow";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const renderRow = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CodexHistoryBackupsRow />
    </QueryClientProvider>,
  );

describe("CodexHistoryBackupsRow (CFG-44)", () => {
  it("shows the size and purges after confirmation, keeping the ledger by default", async () => {
    const empty = {
      migrationBytes: 0,
      restoreLedgerBytes: 2048,
      restoreBackupBytes: 0,
      totalBytes: 2048,
    };
    const api = mockInvoke({
      get_codex_history_backup_usage: {
        migrationBytes: 1024 * 1024,
        restoreLedgerBytes: 2048,
        restoreBackupBytes: 0,
        totalBytes: 1024 * 1024 + 2048,
      },
      purge_codex_history_backups: empty,
    });
    renderRow();

    expect(
      await screen.findByText(
        "1.0 MB on disk: 1.0 MB safe to delete, 2.0 KB restore ledger.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete backups" }));
    expect(
      await screen.findByText("Delete Codex history backups?"),
    ).toBeInTheDocument();
    // The confirm dialog's button shares the label; pick the last one.
    const buttons = screen.getAllByRole("button", { name: "Delete backups" });
    fireEvent.click(buttons[buttons.length - 1]);

    await waitFor(() =>
      expect(api.callsTo("purge_codex_history_backups")).toEqual([
        {
          command: "purge_codex_history_backups",
          args: { includeRestoreLedger: false },
        },
      ]),
    );
  });

  it("offers no purge when nothing is on disk", async () => {
    mockInvoke({
      get_codex_history_backup_usage: {
        migrationBytes: 0,
        restoreLedgerBytes: 0,
        restoreBackupBytes: 0,
        totalBytes: 0,
      },
    });
    renderRow();
    expect(await screen.findByText("No backups on disk.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete backups" })).toBeNull();
  });
});
