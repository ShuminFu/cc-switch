import { describe, expect, it } from "vitest";
import {
  applySessionAnnotations,
  countArchived,
} from "@/components/sessions/sessionAnnotationsFilter";
import type { SessionMeta } from "@/types";

const session = (id: string): SessionMeta => ({
  providerId: "claude",
  sessionId: id,
  sourcePath: `/p/${id}.jsonl`,
  sessionKey: `claude:${id}:/p/${id}.jsonl`,
});

const a = session("a");
const b = session("b");
const c = session("c");
const d = session("d");

describe("applySessionAnnotations (SESS-18)", () => {
  it("keeps the list unchanged without annotations", () => {
    const list = [a, b, c];
    expect(applySessionAnnotations(list, undefined, false)).toBe(list);
    expect(applySessionAnnotations(list, {}, false)).toBe(list);
  });

  it("moves pinned sessions first and hides archived ones", () => {
    const annotations = {
      [c.sessionKey!]: { pinned: true, archived: false, tags: [] },
      [b.sessionKey!]: { pinned: false, archived: true, tags: [] },
      [d.sessionKey!]: { pinned: true, archived: false, tags: ["x"] },
    };
    const ids = (list: SessionMeta[]) => list.map((s) => s.sessionId);
    expect(
      ids(applySessionAnnotations([a, b, c, d], annotations, false)),
    ).toEqual(["c", "d", "a"]);
    expect(
      ids(applySessionAnnotations([a, b, c, d], annotations, true)),
    ).toEqual(["c", "d", "a", "b"]);
    expect(countArchived([a, b, c, d], annotations)).toBe(1);
  });
});
