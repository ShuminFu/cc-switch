import type { SessionMeta } from "@/types";

const haystackCache = new WeakMap<SessionMeta, string>();

function haystack(session: SessionMeta): string {
  let value = haystackCache.get(session);
  if (value === undefined) {
    value = [
      session.sessionId,
      session.title,
      session.summary,
      session.projectDir,
      session.sourcePath,
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    haystackCache.set(session, value);
  }
  return value;
}

const byRecency = (a: SessionMeta, b: SessionMeta) =>
  (b.lastActiveAt ?? b.createdAt ?? 0) - (a.lastActiveAt ?? a.createdAt ?? 0);

/**
 * Metadata search: every whitespace-separated term must appear as a
 * substring of the session's id / title / summary / directory / path.
 * Linear over a per-session cached haystack, so there is no index to rebuild
 * when the list refreshes or the provider filter changes (SESS-21).
 */
export function filterSessionsByQuery(
  sessions: SessionMeta[],
  query: string,
): SessionMeta[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const matches =
    terms.length === 0
      ? [...sessions]
      : sessions.filter((session) => {
          const text = haystack(session);
          return terms.every((term) => text.includes(term));
        });
  return matches.sort(byRecency);
}
