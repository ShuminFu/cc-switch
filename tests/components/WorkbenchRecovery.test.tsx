import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { RecoveryBackupList } from "@/components/workbench/RecoveryBackupList";
import { server } from "../msw/server";
import { readJson as json } from "./workbenchTestUtils";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe("RecoveryBackupList (PRODUCT-72)", () => {
  it("offers only compatible backups and restores the chosen one", async () => {
    const restored: unknown[] = [];
    server.use(
      http.post("http://tauri.local/list_recovery_backups", () =>
        HttpResponse.json([
          {
            filename: "db_premigration_20260101_000000.db",
            sizeBytes: 2048,
            createdAt: "2026-01-01T00:00:00Z",
            schemaVersion: 22,
            compatible: true,
          },
          {
            filename: "db_backup_20260102_000000.db",
            sizeBytes: 2048,
            createdAt: "2026-01-02T00:00:00Z",
            schemaVersion: 23,
            compatible: false,
          },
        ]),
      ),
      http.post(
        "http://tauri.local/restore_backup_for_recovery",
        async ({ request }) => {
          restored.push(await json(request));
          return HttpResponse.json("db_too_new_v23.db");
        },
      ),
    );
    render(<RecoveryBackupList />);
    expect(
      await screen.findByText("db_premigration_20260101_000000.db"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("db_backup_20260102_000000.db"),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: /Restore and restart/ }),
    );
    await waitFor(() =>
      expect(restored).toEqual([
        { filename: "db_premigration_20260101_000000.db" },
      ]),
    );
  });
});
