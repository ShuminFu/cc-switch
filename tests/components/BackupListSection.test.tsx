import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { parseBackupName } from "@/components/settings/backupNames";

const backups = [
  "db_backup_20260101_101500.db",
  "db_manual_20260102_111500.db",
  "db_safety_20260103_121500_2.db",
  "db_premigration_20260104_131500.db",
  "before upgrade.db",
].map((filename) => ({
  filename,
  sizeBytes: 2048,
  createdAt: "2026-01-01T10:15:00Z",
}));

vi.mock("@/hooks/useBackupManager", () => ({
  useBackupManager: () => ({
    backups,
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

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      (options?.defaultValue as string) ?? key,
  }),
}));

describe("parseBackupName", () => {
  it.each([
    ["db_backup_20260101_101500.db", "auto", "2026-01-01 10:15:00"],
    ["db_manual_20260102_111500.db", "manual", "2026-01-02 11:15:00"],
    ["db_safety_20260103_121500_2.db", "safety", "2026-01-03 12:15:00"],
    [
      "db_premigration_20260104_131500.db",
      "premigration",
      "2026-01-04 13:15:00",
    ],
    ["before upgrade.db", null, "before upgrade"],
    ["db_manual_bad.db", null, "db_manual_bad"],
  ])("%s", (filename, kind, displayName) => {
    expect(parseBackupName(filename)).toEqual({ kind, displayName });
  });
});

describe("BackupListSection", () => {
  it("labels non-auto backup kinds instead of showing raw names", () => {
    const { container } = render(
      <BackupListSection onSettingsChange={vi.fn()} />,
    );

    expect(screen.getByText("Manual")).toBeInTheDocument();
    expect(screen.getByText("Safety")).toBeInTheDocument();
    expect(screen.getByText("Pre-migration")).toBeInTheDocument();
    expect(screen.getByText("2026-01-03 12:15:00")).toBeInTheDocument();
    expect(screen.queryByText(/db_(manual|safety|premigration)_/)).toBeNull();
    // Auto backups and user-named backups get no badge.
    expect(container.querySelectorAll("[data-backup-kind]")).toHaveLength(3);
  });
});
