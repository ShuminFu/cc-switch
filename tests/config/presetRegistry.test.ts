import { describe, expect, it } from "vitest";
import {
  buildPresetEntries,
  findPresetEntry,
  getPresetEntries,
  slugifyPresetName,
} from "@/config/presetRegistry";
import { ALL_APP_IDS } from "@/config/apps";
import { providerPresets } from "@/config/claudeProviderPresets";

describe("preset registry (FE-PROV-19)", () => {
  it("gives every preset of every app a unique, app-scoped id", () => {
    for (const appId of ALL_APP_IDS) {
      const entries = getPresetEntries(appId);
      const ids = entries.map((entry) => entry.id);
      expect(new Set(ids).size).toBe(ids.length);
      for (const id of ids) {
        expect(id.startsWith(`${appId}:`)).toBe(true);
        expect(id).not.toMatch(/^[a-z-]+-\d+$/);
      }
    }
  });

  it("keeps ids stable when presets are reordered or inserted", () => {
    const a = { name: "Alpha Relay" };
    const b = { name: "Beta" };
    const before = buildPresetEntries("codex", [a, b]);
    const after = buildPresetEntries("codex", [{ name: "New One" }, b, a]);
    const idOf = (entries: typeof before, preset: object) =>
      entries.find((entry) => entry.preset === preset)?.id;
    expect(idOf(after, a)).toBe(idOf(before, a));
    expect(idOf(after, b)).toBe(idOf(before, b));
    expect(idOf(before, a)).toBe("codex:alpha-relay");
  });

  it("disambiguates duplicate names and keeps non-Latin names", () => {
    const entries = buildPresetEntries("claude", [
      { name: "Dup" },
      { name: "Dup" },
      { name: "火山 Agent" },
    ]);
    expect(entries.map((entry) => entry.id)).toEqual([
      "claude:dup",
      "claude:dup-2",
      "claude:火山-agent",
    ]);
    expect(slugifyPresetName("  --  ")).toBe("");
  });

  it("resolves Claude presets despite hidden presets earlier in the list", () => {
    const visible = providerPresets.filter((preset) => !preset.hidden);
    const last = visible[visible.length - 1];
    const entry = getPresetEntries("claude").find(
      (candidate) => candidate.preset === last,
    );
    expect(entry).toBeDefined();
    expect(findPresetEntry("claude", entry!.id)?.preset).toBe(last);
    expect(findPresetEntry("claude", "custom")).toBeUndefined();
    expect(findPresetEntry("codex", entry!.id)).toBeUndefined();
  });

  it("only offers non-official Codex presets to Grok Build", () => {
    for (const { preset } of getPresetEntries("grokbuild")) {
      expect(preset.category).not.toBe("official");
      expect(preset.isOfficial).not.toBe(true);
    }
  });
});
