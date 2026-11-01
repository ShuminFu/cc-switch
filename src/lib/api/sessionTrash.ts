import { invoke } from "./invoke";

/** One path moved to the session trash (SESS-41). */
export interface TrashedItem {
  originalPath: string;
  storedName: string;
}

/** A deleted file-based session kept in the trash for 30 days. */
export interface SessionTrashEntry {
  id: string;
  providerId: string;
  sessionId: string;
  deletedAt: number;
  items: TrashedItem[];
}

export const sessionTrashApi = {
  list(): Promise<SessionTrashEntry[]> {
    return invoke<SessionTrashEntry[]>("list_session_trash");
  },

  restore(id: string): Promise<SessionTrashEntry> {
    return invoke<SessionTrashEntry>("restore_session_from_trash", { id });
  },

  /** Restore the most recently trashed copy of a session ("Undo" on delete). */
  async undoDelete(
    providerId: string,
    sessionId: string,
  ): Promise<SessionTrashEntry> {
    const entries = await sessionTrashApi.list();
    const entry = entries.find(
      (e) => e.providerId === providerId && e.sessionId === sessionId,
    );
    if (!entry) {
      throw new Error("This session is not in the trash");
    }
    return sessionTrashApi.restore(entry.id);
  },
};
