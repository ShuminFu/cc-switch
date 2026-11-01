import { describe, expect, it } from "vitest";
import {
  classify,
  parseFindings,
  render,
} from "../../scripts/roadmap-status.mjs";

const findingsFile = `# Testing

### TEST-01: First finding

**Type:** test · **Priority:** P1 · **Effort:** S

### TEST-02: Second | with pipe

**Type:** test · **Priority:** P3 · **Effort:** M

### TEST-03: Third

**Type:** test · **Priority:** P2 · **Effort:** M
`;

describe("scripts/roadmap-status.mjs", () => {
  const findings = parseFindings([["testing", findingsFile]]);
  const known = new Set(findings.map((f) => f.id));

  it("parses IDs, titles and priorities", () => {
    expect(findings).toEqual([
      {
        id: "TEST-01",
        title: "First finding",
        area: "testing",
        priority: "P1",
      },
      {
        id: "TEST-02",
        title: "Second | with pipe",
        area: "testing",
        priority: "P3",
      },
      { id: "TEST-03", title: "Third", area: "testing", priority: "P2" },
    ]);
  });

  it("classifies subject and body citations, ignoring merges", () => {
    const result = classify(known, [
      { hash: "aaa", subject: "fix: thing (TEST-01)", body: "" },
      {
        hash: "bbb",
        subject: "test: more",
        body: "Covers TEST-02 as a first increment; the rest later.",
      },
      { hash: "ccc", subject: "Merge wave (TEST-03)", body: "" },
      {
        hash: "ddd",
        subject: "fix: roll back partial sort updates (TEST-01)",
        body: "",
      },
    ]);
    expect(result.get("TEST-01")).toEqual({
      status: "DONE",
      commits: ["aaa", "ddd"],
      note: "",
    });
    expect(result.get("TEST-02")?.status).toBe("PARTIAL");
    expect(result.has("TEST-03")).toBe(false);
  });

  it("renders overrides, escapes pipes and counts statuses", () => {
    const classified = classify(known, [
      { hash: "aaa", subject: "fix: thing (TEST-01)", body: "" },
    ]);
    const md = render(findings, classified, {});
    expect(md).toContain(
      "| TEST-01 | P1 | testing | First finding | DONE | aaa |  |",
    );
    expect(md).toContain("Second \\| with pipe | OPEN");
    expect(md).toContain("3 findings · DONE: 1 · OPEN: 2");

    const withOverride = render(findings, classified, {
      "TEST-03": { status: "SKIPPED", note: "needs hardware" },
    });
    expect(withOverride).toContain(
      "| TEST-03 | P2 | testing | Third | SKIPPED |  | needs hardware |",
    );
  });
});
