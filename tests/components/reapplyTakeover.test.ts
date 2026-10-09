import { describe, expect, it } from "vitest";
import { summarizeReapply } from "@/components/proxy/reapplyTakeover";

describe("summarizeReapply", () => {
  it("reports nothing when no app is taken over", () => {
    expect(summarizeReapply([])).toEqual({ kind: "nothing" });
  });

  it("lists the rebuilt apps when every app succeeded", () => {
    expect(
      summarizeReapply([
        { appType: "claude", ok: true },
        { appType: "codex", ok: true },
      ]),
    ).toEqual({ kind: "ok", apps: ["claude", "codex"] });
  });

  it("separates failures from successes", () => {
    expect(
      summarizeReapply([
        { appType: "claude", ok: true },
        { appType: "gemini", ok: false, error: "backup missing" },
      ]),
    ).toEqual({
      kind: "partial",
      okApps: ["claude"],
      failures: [{ appType: "gemini", ok: false, error: "backup missing" }],
    });
  });
});
