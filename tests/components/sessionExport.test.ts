import { describe, expect, it } from "vitest";
import type { SessionMessage, SessionMeta } from "@/types";
import {
  sessionExportFileName,
  sessionToJson,
  sessionToMarkdown,
} from "@/components/session-export/sessionExport";

const meta: SessionMeta = {
  providerId: "claude",
  sessionId: "1a2b3c4d-5e6f",
  title: "Fix login bug",
  projectDir: "/work/app",
  createdAt: Date.UTC(2026, 0, 2, 3, 4, 5),
  lastActiveAt: Date.UTC(2026, 0, 2, 4, 0, 0),
  sourcePath: "/home/u/.claude/projects/app/1a2b.jsonl",
};

const messages: SessionMessage[] = [
  {
    role: "user",
    content: "Why does login fail?",
    ts: Date.UTC(2026, 0, 2, 3, 4, 6),
  },
  { role: "assistant", content: "Let me check `auth.ts`." },
  {
    role: "tool",
    content: 'read_file {"path":"auth.ts"}\n```ts\nx\n```',
    ts: Date.UTC(2026, 0, 2, 3, 4, 7),
  },
];

describe("session export (PRODUCT-48)", () => {
  it("renders Markdown with metadata, role headers, timestamps and fenced tool output", () => {
    const md = sessionToMarkdown(meta, messages);
    expect(md).toContain("# Fix login bug");
    expect(md).toContain("- **Project:** /work/app");
    expect(md).toContain("- **Created:** 2026-01-02T03:04:05.000Z");
    expect(md).toContain("## User · 2026-01-02T03:04:06.000Z");
    expect(md).toContain("## Assistant\n\nLet me check `auth.ts`.");
    // The tool body contains a ``` fence, so the wrapper must be longer.
    expect(md).toContain(
      '## Tool · 2026-01-02T03:04:07.000Z\n\n````\nread_file {"path":"auth.ts"}\n```ts\nx\n```\n````',
    );
  });

  it("round-trips messages through JSON", () => {
    const parsed = JSON.parse(
      sessionToJson(meta, messages, new Date(Date.UTC(2026, 5, 1))),
    );
    expect(parsed.format).toBe("cc-switch.session");
    expect(parsed.exportedAt).toBe("2026-06-01T00:00:00.000Z");
    expect(parsed.session).toEqual(meta);
    expect(parsed.messages).toEqual(messages);
  });

  it("builds a safe default file name", () => {
    expect(sessionExportFileName(meta, "markdown")).toBe(
      "claude-fix-login-bug-1a2b3c4d.md",
    );
    expect(
      sessionExportFileName(
        { providerId: "codex", sessionId: "../../x", title: "a/b" },
        "json",
      ),
    ).toBe("codex-a-b-x.json");
  });

  it("formats 100 sessions quickly", () => {
    const many = Array.from({ length: 200 }, (_, i) => ({
      role: i % 2 ? "assistant" : "user",
      content: `message ${i} `.repeat(50),
      ts: i * 1000 + 1,
    }));
    const start = performance.now();
    for (let i = 0; i < 100; i++) sessionToMarkdown(meta, many);
    expect(performance.now() - start).toBeLessThan(5000);
  });
});
