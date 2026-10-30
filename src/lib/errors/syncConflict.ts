import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * A manual upload/download sent with `force: false` was refused because the
 * other side changed since the last successful sync.
 * - `remote_changed`: another device uploaded; uploading would overwrite it.
 * - `local_changed`: local data changed; downloading would overwrite it.
 */
export type SyncConflictKind = "remote_changed" | "local_changed";

/** Backend error keys (`services/sync_protocol.rs`). */
export const SYNC_CONFLICT_KEYS: Record<SyncConflictKind, string> = {
  remote_changed: "sync.conflict.remote_changed",
  local_changed: "sync.conflict.local_changed",
};

// `AppError::Localized` reaches the frontend as "{zh} ({en})" without its
// key, so the conflict is recognised by the key when present and otherwise
// by the fixed start of the zh / en messages.
const CONFLICT_MARKERS: Record<SyncConflictKind, readonly string[]> = {
  remote_changed: [
    SYNC_CONFLICT_KEYS.remote_changed,
    "the remote data changed since the last sync",
    "远端数据自上次同步后已被其他设备更新",
  ],
  local_changed: [
    SYNC_CONFLICT_KEYS.local_changed,
    "local data changed since the last sync",
    "本地数据自上次同步后已修改",
  ],
};

export function detectSyncConflict(error: unknown): SyncConflictKind | null {
  const message = extractErrorMessage(error);
  if (!message) return null;
  for (const kind of Object.keys(CONFLICT_MARKERS) as SyncConflictKind[]) {
    if (CONFLICT_MARKERS[kind].some((marker) => message.includes(marker))) {
      return kind;
    }
  }
  return null;
}
