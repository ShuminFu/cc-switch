import { describe, expect, it } from "vitest";
import {
  APP_PROFILE_SCOPE,
  hasScopeSnapshot,
} from "@/components/profiles/scope";
import type { Profile } from "@/lib/api/profiles";

const emptySlots = {
  claude: null,
  "claude-desktop": null,
  codex: null,
  gemini: null,
  grokbuild: null,
};

const profile = (overrides: Partial<Profile["payload"]> = {}): Profile => ({
  id: "p",
  name: "P",
  payload: {
    providers: { ...emptySlots },
    mcp: { ...emptySlots },
    skills: { ...emptySlots },
    prompts: { ...emptySlots },
    ...overrides,
  },
});

describe("profile scope mirror", () => {
  it("maps every app with a current provider to its own scope", () => {
    expect(APP_PROFILE_SCOPE).toEqual({
      claude: "claude",
      "claude-desktop": "claude-desktop",
      codex: "codex",
      gemini: "gemini",
      grokbuild: "grokbuild",
    });
    expect(APP_PROFILE_SCOPE.opencode).toBeUndefined();
    expect(APP_PROFILE_SCOPE.hermes).toBeUndefined();
  });

  it("detects per-scope snapshots for gemini and grokbuild", () => {
    const geminiOnly = profile({
      providers: { ...emptySlots, gemini: "g1" },
    });
    expect(hasScopeSnapshot(geminiOnly, "gemini")).toBe(true);
    expect(hasScopeSnapshot(geminiOnly, "grokbuild")).toBe(false);
    expect(hasScopeSnapshot(geminiOnly, "claude")).toBe(false);

    const grokPrompts = profile({
      prompts: { ...emptySlots, grokbuild: "pr1" },
    });
    expect(hasScopeSnapshot(grokPrompts, "grokbuild")).toBe(true);
    expect(hasScopeSnapshot(grokPrompts, "codex")).toBe(false);
  });

  it("treats snapshots saved before the new slots existed as uncaptured", () => {
    const legacy = {
      id: "old",
      name: "Old",
      payload: {
        providers: { claude: "p1", "claude-desktop": null, codex: null },
        mcp: { claude: ["m1"], "claude-desktop": null, codex: null },
        skills: { claude: null, "claude-desktop": null, codex: null },
        prompts: { claude: null, "claude-desktop": null, codex: null },
      },
    } as unknown as Profile;
    expect(hasScopeSnapshot(legacy, "claude")).toBe(true);
    expect(hasScopeSnapshot(legacy, "gemini")).toBe(false);
    expect(hasScopeSnapshot(legacy, "grokbuild")).toBe(false);
  });
});
