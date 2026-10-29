import { describe, expect, it } from "vitest";
import { OPENCLAW_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { openclawProviderFormSchema } from "@/features/providers/forms/openclaw/schema";
import { openclawFormFromProvider } from "@/features/providers/forms/openclaw/fromProvider";
import {
  OPENCLAW_PRESET_ENTRIES,
  openclawPresetEntries,
} from "@/features/providers/forms/openclaw/presets";
import {
  toOpenClawProviderMeta,
  toOpenClawSettingsConfig,
  toOpenClawSuggestedDefaults,
} from "@/features/providers/forms/openclaw/toSettingsConfig";

describe("openclaw form module (CCS-192)", () => {
  it("schema is the openclaw branch of the union", () => {
    expect(
      openclawProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "openclaw",
      }).success,
    ).toBe(true);
  });

  it("starts a new provider from the default config", () => {
    expect(
      openclawFormFromProvider(undefined).defaultValues.settingsConfig,
    ).toBe(OPENCLAW_DEFAULT_CONFIG);
  });

  it("saves the trimmed editor text and the shared meta", () => {
    expect(
      toOpenClawSettingsConfig({
        name: "R",
        settingsConfig: ' {"api":"openai-completions"} ',
      }),
    ).toBe('{"api":"openai-completions"}');
    const initialData = { meta: { costMultiplier: "2" } };
    expect(
      toOpenClawProviderMeta(
        initialData,
        openclawFormFromProvider(initialData),
      ),
    ).toMatchObject({ costMultiplier: "2", isFullUrl: undefined });
  });

  it("rebases suggested model refs onto the submitted provider key", () => {
    const defaults = {
      model: { primary: "preset/kimi-k2", fallbacks: ["preset/kimi-k2-mini"] },
      modelCatalog: { "preset/kimi-k2": { alias: "Kimi" } },
    };
    expect(toOpenClawSuggestedDefaults(defaults, "relay")).toEqual({
      model: { primary: "relay/kimi-k2", fallbacks: ["relay/kimi-k2-mini"] },
      modelCatalog: { "relay/kimi-k2": { alias: "Kimi" } },
    });
    expect(toOpenClawSuggestedDefaults(defaults, undefined)).toBe(defaults);
    expect(toOpenClawSuggestedDefaults(undefined, "relay")).toBeUndefined();
  });

  it("ids presets by app and index", () => {
    expect(OPENCLAW_PRESET_ENTRIES).toHaveLength(
      openclawProviderPresets.length,
    );
    expect(openclawPresetEntries(openclawProviderPresets.slice(0, 1))).toEqual([
      { id: "openclaw-0", preset: openclawProviderPresets[0] },
    ]);
  });
});
