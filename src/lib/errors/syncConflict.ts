import { getErrorCode } from "@/lib/errors/appError";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * A manual upload/download sent with `force: false` was refused because the
 * other side changed since the last successful sync.
 * - `remote_changed`: another device uploaded; uploading would overwrite it.
 * - `local_changed`: local data changed; downloading would overwrite it.
 */
export type SyncConflictKind = "remote_changed" | "local_changed";

/** Backend error codes (`services/sync_protocol.rs`). */
export const SYNC_CONFLICT_KEYS: Record<SyncConflictKind, string> = {
  remote_changed: "sync.conflict.remote_changed",
  local_changed: "sync.conflict.local_changed",
};

const KIND_BY_CODE = new Map<string, SyncConflictKind>(
  (Object.keys(SYNC_CONFLICT_KEYS) as SyncConflictKind[]).map((kind) => [
    SYNC_CONFLICT_KEYS[kind],
    kind,
  ]),
);

export function detectSyncConflict(error: unknown): SyncConflictKind | null {
  // The sync commands return a structured `AppError` whose `code` is the key.
  const code = getErrorCode(error);
  if (code) return KIND_BY_CODE.get(code) ?? null;

  // Legacy: a plain string error that embeds the key.
  const message = extractErrorMessage(error);
  if (!message) return null;
  for (const [key, kind] of KIND_BY_CODE) {
    if (message.includes(key)) return kind;
  }
  return null;
}
