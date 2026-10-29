import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/button";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { findUnnamedButtons } from "../a11y/buttonName";

vi.mock("@/hooks/useBackupManager", () => ({
  useBackupManager: () => ({
    backups: [
      {
        filename: "db_backup_20260101_120000.db",
        createdAt: "2026-01-01T12:00:00Z",
        sizeBytes: 2048,
      },
    ],
    isLoading: false,
    create: vi.fn(),
    isCreating: false,
    restore: vi.fn(),
    isRestoring: false,
    rename: vi.fn(),
    isRenaming: false,
    remove: vi.fn(),
    isDeleting: false,
  }),
}));

describe("icon-only buttons have accessible names", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("names the confirm/cancel buttons in BackupListSection rename mode", async () => {
    const { container } = render(
      <BackupListSection onSettingsChange={() => {}} />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.backupManager.rename" }),
    );

    expect(
      screen.getByRole("button", { name: "common.confirm" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeInTheDocument();
    expect(await findUnnamedButtons(container)).toEqual([]);
  });

  it("warns in dev when an icon button has no accessible name", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    render(
      <Button size="icon">
        <svg aria-hidden="true" />
      </Button>,
    );

    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0][0])).toContain("accessible name");
  });

  it("does not warn for labelled or titled icon buttons", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    render(
      <>
        <Button size="icon" aria-label="Delete">
          <svg aria-hidden="true" />
        </Button>
        <Button size="icon" title="Edit">
          <svg aria-hidden="true" />
        </Button>
        <Button size="icon">
          <span className="sr-only">Toggle</span>
        </Button>
      </>,
    );

    expect(warn).not.toHaveBeenCalled();
  });
});
