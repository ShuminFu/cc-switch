import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DatabaseUpgrade } from "@/components/DatabaseUpgrade";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

const askMock = vi.fn();

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: (...args: unknown[]) => askMock(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  exit: vi.fn(),
}));

const payload = {
  path: "/home/u/.cc-switch/cc-switch.db",
  error: "too new",
  kind: "db_version_too_new",
  db_version: 16,
  supported_version: 15,
  restorable_backups: [
    {
      filename: "db_backup_20260101_000000.db",
      sizeBytes: 4096,
      createdAt: "2026-01-01T00:00:00Z",
      dbVersion: 15,
    },
  ],
};

describe("DatabaseUpgrade downgrade restore", () => {
  let calls: { command: string; body: unknown }[];

  beforeEach(() => {
    calls = [];
    const record = (command: string, response: string | boolean | null) =>
      http.post(`${TAURI_ENDPOINT}/${command}`, async ({ request }) => {
        calls.push({ command, body: await request.json() });
        return HttpResponse.json(response);
      });
    server.use(
      record("check_app_update_available", null),
      record("restore_backup_over_newer_db", "db_newer_v16_x.sqlite"),
      record("restart_app", true),
    );
  });

  it("lists restorable backups and restores the chosen one after confirmation", async () => {
    askMock.mockResolvedValue(true);
    render(<DatabaseUpgrade payload={payload} />);

    expect(
      screen.getByText("db_backup_20260101_000000.db"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "dbUpgrade.restoreBackup" }),
    );

    await waitFor(() =>
      expect(calls.map((c) => c.command)).toContain("restart_app"),
    );
    expect(askMock).toHaveBeenCalledTimes(1);
    expect(
      calls.find((c) => c.command === "restore_backup_over_newer_db")?.body,
    ).toEqual({ filename: "db_backup_20260101_000000.db" });
  });

  it("does nothing when the restore is not confirmed", async () => {
    askMock.mockResolvedValue(false);
    render(<DatabaseUpgrade payload={payload} />);

    fireEvent.click(
      screen.getByRole("button", { name: "dbUpgrade.restoreBackup" }),
    );

    await waitFor(() => expect(askMock).toHaveBeenCalledTimes(1));
    expect(calls.map((c) => c.command)).not.toContain(
      "restore_backup_over_newer_db",
    );
  });

  it("hides the restore section when no backup is restorable", () => {
    render(
      <DatabaseUpgrade payload={{ ...payload, restorable_backups: [] }} />,
    );
    expect(screen.queryByText("dbUpgrade.restoreTitle")).toBeNull();
  });
});
