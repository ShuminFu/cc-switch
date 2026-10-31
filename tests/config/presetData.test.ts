import { describe, expect, it, vi } from "vitest";
import geminiPresetData from "@/config/presets/gemini.json";
import {
  geminiPresetSchema,
  loadPresetTable,
  validatePresetTable,
} from "@/config/presets/schema";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";

describe("presets as data (FE-ARCH-34)", () => {
  it("the shipped Gemini table validates without issues", () => {
    const { presets, issues } = validatePresetTable(
      geminiPresetSchema,
      geminiPresetData,
    );
    expect(issues).toEqual([]);
    expect(presets).toHaveLength(geminiPresetData.length);
  });

  it("geminiProviderPresets is the validated JSON table", () => {
    expect(geminiProviderPresets).toEqual(geminiPresetData);
    const names = geminiProviderPresets.map((p) => p.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("skips malformed entries instead of failing the whole table", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const loaded = loadPresetTable("gemini", geminiPresetSchema, [
      { name: "Good", websiteUrl: "", settingsConfig: { env: {} } },
      { name: "No config", websiteUrl: "" },
      {
        name: "Typo",
        websiteUrl: "",
        settingsConfig: { env: {} },
        ispartner: true,
      },
      {
        name: "Bad env",
        websiteUrl: "",
        settingsConfig: { env: { GEMINI_MODEL: 1 } },
      },
    ]);
    expect(loaded.map((p) => p.name)).toEqual(["Good"]);
    expect(warn).toHaveBeenCalledTimes(3);
    warn.mockRestore();
  });

  it("a non-array table yields no presets", () => {
    const { presets, issues } = validatePresetTable(geminiPresetSchema, {});
    expect(presets).toEqual([]);
    expect(issues).toHaveLength(1);
  });
});
