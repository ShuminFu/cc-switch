/**
 * Backup file names written by the backend (`database/backup.rs`,
 * `BackupKind::prefix`): `<prefix>YYYYMMDD_HHMMSS[_n].db`. Renamed backups
 * keep whatever name the user chose and have no kind.
 */
export type BackupKind = "auto" | "manual" | "safety" | "premigration";

const PREFIX_TO_KIND: Record<string, BackupKind> = {
  db_backup_: "auto",
  db_manual_: "manual",
  db_safety_: "safety",
  db_premigration_: "premigration",
};

const BACKUP_NAME_RE =
  /^(db_backup_|db_manual_|db_safety_|db_premigration_)(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})(?:_\d+)?\.db$/;

export interface ParsedBackupName {
  /** `null` for user-named backups. */
  kind: BackupKind | null;
  /** `YYYY-MM-DD HH:MM:SS` for generated names, else the name without `.db`. */
  displayName: string;
}

export function parseBackupName(filename: string): ParsedBackupName {
  const match = filename.match(BACKUP_NAME_RE);
  if (!match) {
    return { kind: null, displayName: filename.replace(/\.db$/, "") };
  }
  const [, prefix, y, m, d, hh, mm, ss] = match;
  return {
    kind: PREFIX_TO_KIND[prefix],
    displayName: `${y}-${m}-${d} ${hh}:${mm}:${ss}`,
  };
}
