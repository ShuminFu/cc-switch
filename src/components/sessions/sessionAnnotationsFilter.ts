import type { SessionMeta } from "@/types";
import type { SessionAnnotations } from "@/lib/api/sessionAnnotations";
import { getSessionKey } from "./utils";

/**
 * Apply pin/archive annotations to a session list (SESS-18): archived
 * sessions are hidden unless `showArchived`, and pinned sessions move to the
 * front. The order is otherwise kept (stable).
 */
export function applySessionAnnotations(
  sessions: SessionMeta[],
  annotations: SessionAnnotations | undefined,
  showArchived: boolean,
): SessionMeta[] {
  if (!annotations || Object.keys(annotations).length === 0) return sessions;
  const visible = showArchived
    ? sessions
    : sessions.filter(
        (session) => !annotations[getSessionKey(session)]?.archived,
      );
  const pinned: SessionMeta[] = [];
  const rest: SessionMeta[] = [];
  for (const session of visible) {
    (annotations[getSessionKey(session)]?.pinned ? pinned : rest).push(session);
  }
  return pinned.length === 0 ? visible : [...pinned, ...rest];
}

/** Number of archived sessions in the list (for the "show archived" toggle). */
export function countArchived(
  sessions: SessionMeta[],
  annotations: SessionAnnotations | undefined,
): number {
  if (!annotations) return 0;
  return sessions.filter(
    (session) => annotations[getSessionKey(session)]?.archived,
  ).length;
}
