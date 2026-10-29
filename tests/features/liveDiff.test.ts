import { describe, expect, it } from "vitest";
import {
  collapseUnchanged,
  diffLines,
  hasChanges,
  splitLines,
} from "@/features/providers/forms/liveDiff";

describe("liveDiff (CCS-205)", () => {
  it("splits lines without a trailing empty line and normalises CRLF", () => {
    expect(splitLines("a\r\nb\n")).toEqual(["a", "b"]);
    expect(splitLines(null)).toEqual([]);
  });

  it("pairs replaced lines and keeps line numbers per side", () => {
    const rows = diffLines("a\nb\nc\n", "a\nB\nc\nd\n");
    expect(rows.map((row) => row.kind)).toEqual([
      "same",
      "changed",
      "same",
      "added",
    ]);
    expect(rows[1]).toEqual({
      kind: "changed",
      left: { lineNo: 2, text: "b" },
      right: { lineNo: 2, text: "B" },
    });
    expect(rows[3].left).toBeUndefined();
    expect(rows[3].right).toEqual({ lineNo: 4, text: "d" });
  });

  it("treats a missing file as all added and a deletion as all removed", () => {
    expect(diffLines(null, "x\ny").map((row) => row.kind)).toEqual([
      "added",
      "added",
    ]);
    expect(diffLines("x", null).map((row) => row.kind)).toEqual(["removed"]);
    expect(hasChanges(diffLines("same", "same"))).toBe(false);
  });

  it("aligns insertions in the middle via LCS", () => {
    const rows = diffLines("1\n2\n3\n4", "1\n2\nnew\n3\n4");
    expect(rows.map((row) => row.kind)).toEqual([
      "same",
      "same",
      "added",
      "same",
      "same",
    ]);
  });

  it("collapses unchanged runs outside the context window", () => {
    const before = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
    const after = before.replace("line 10", "line ten");
    const segments = collapseUnchanged(diffLines(before, after), 2);
    expect(segments.map((segment) => segment.type)).toEqual([
      "skipped",
      "rows",
      "skipped",
    ]);
    expect(segments[0]).toEqual({ type: "skipped", count: 8 });
    expect(segments[2]).toEqual({ type: "skipped", count: 7 });
  });
});
