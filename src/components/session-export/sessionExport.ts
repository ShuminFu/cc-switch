import type { SessionMessage, SessionMeta } from "@/types";

/**
 * Session export formatters (PRODUCT-48). Pure functions so they can be
 * tested and reused for single and batch exports.
 */

export type SessionExportFormat = "markdown" | "json";

const iso = (ts?: number | null) =>
  typeof ts === "number" && Number.isFinite(ts) && ts > 0
    ? new Date(ts).toISOString()
    : undefined;

const roleLabel = (role: string) => {
  const r = role.trim();
  if (!r) return "Message";
  return r.charAt(0).toUpperCase() + r.slice(1);
};

/** A code fence longer than any backtick run inside `content`. */
const fenceFor = (content: string) => {
  const longest = Math.max(
    0,
    ...Array.from(content.matchAll(/`+/g), (m) => m[0].length),
  );
  return "`".repeat(Math.max(3, longest + 1));
};

/** Tool calls/results are machine text: fence them so Markdown keeps them verbatim. */
const isMachineRole = (role: string) =>
  /^(tool|function|tool_result|tool_use|system)$/i.test(role.trim());

export function sessionTitle(meta: SessionMeta): string {
  return (
    meta.title?.trim() ||
    meta.summary?.trim().split("\n")[0] ||
    meta.sessionId ||
    "Session"
  );
}

export function sessionToMarkdown(
  meta: SessionMeta,
  messages: SessionMessage[],
): string {
  const lines: string[] = [`# ${sessionTitle(meta)}`, ""];
  const fields: Array<[string, string | undefined | null]> = [
    ["Session", meta.sessionId],
    ["App", meta.providerId],
    ["Project", meta.projectDir],
    ["Created", iso(meta.createdAt)],
    ["Last active", iso(meta.lastActiveAt)],
    ["Source", meta.sourcePath],
  ];
  for (const [label, value] of fields) {
    if (value) lines.push(`- **${label}:** ${value}`);
  }
  lines.push("", "---", "");

  for (const message of messages) {
    const when = iso(message.ts);
    lines.push(`## ${roleLabel(message.role)}${when ? ` · ${when}` : ""}`, "");
    const content = message.content ?? "";
    if (isMachineRole(message.role)) {
      const fence = fenceFor(content);
      lines.push(fence, content, fence);
    } else {
      lines.push(content);
    }
    lines.push("");
  }
  return lines.join("\n");
}

export interface SessionExportJson {
  format: "cc-switch.session";
  version: 1;
  exportedAt: string;
  session: SessionMeta;
  messages: SessionMessage[];
}

export function sessionToJsonObject(
  meta: SessionMeta,
  messages: SessionMessage[],
  now: Date = new Date(),
): SessionExportJson {
  return {
    format: "cc-switch.session",
    version: 1,
    exportedAt: now.toISOString(),
    session: meta,
    messages,
  };
}

export function sessionToJson(
  meta: SessionMeta,
  messages: SessionMessage[],
  now?: Date,
): string {
  return JSON.stringify(sessionToJsonObject(meta, messages, now), null, 2);
}

/** Safe default file name such as `claude-fix-login-bug-1a2b3c4d.md`. */
export function sessionExportFileName(
  meta: SessionMeta,
  format: SessionExportFormat,
): string {
  const slug = sessionTitle(meta)
    .toLowerCase()
    .replace(/[^a-z0-9一-鿿]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
  const id = (meta.sessionId || "session")
    .replace(/[^a-zA-Z0-9]/g, "")
    .slice(0, 8);
  const base = [meta.providerId, slug, id].filter(Boolean).join("-");
  return `${base || "session"}.${format === "json" ? "json" : "md"}`;
}

export function formatSession(
  meta: SessionMeta,
  messages: SessionMessage[],
  format: SessionExportFormat,
): string {
  return format === "json"
    ? sessionToJson(meta, messages)
    : sessionToMarkdown(meta, messages);
}
