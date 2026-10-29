import { describe, expect, it } from "vitest";
import { HERMES_DEFAULT_CONFIG } from "@/components/providers/forms/hooks/useHermesFormState";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { hermesProviderFormSchema } from "@/features/providers/forms/hermes/schema";
import { hermesFormFromProvider } from "@/features/providers/forms/hermes/fromProvider";
import {
  HERMES_PRESET_ENTRIES,
  hermesPresetEntries,
} from "@/features/providers/forms/hermes/presets";
import {
  toHermesProviderMeta,
  toHermesSettingsConfig,
} from "@/features/providers/forms/hermes/toSettingsConfig";

const values = {
  name: "Relay",
  websiteUrl: "",
  settingsConfig: '  {"name":"relay"}\n',
};

describe("hermes form module (CCS-192)", () => {
  it("schema is the hermes branch of the union", () => {
    expect(
      hermesProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "hermes",
      }).success,
    ).toBe(true);
    expect(
      hermesProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "openclaw",
      }).success,
    ).toBe(false);
  });

  it("starts a new provider from the default config", () => {
    const state = hermesFormFromProvider(undefined);
    expect(state.defaultValues.settingsConfig).toBe(HERMES_DEFAULT_CONFIG);
    expect(state.endpointAutoSelect).toBe(true);
  });

  it("opens a saved provider with its settings", () => {
    const state = hermesFormFromProvider({
      name: "Relay",
      settingsConfig: { name: "relay", base_url: "https://r.example.com" },
    });
    expect(state.defaultValues.name).toBe("Relay");
    expect(JSON.parse(state.defaultValues.settingsConfig)).toEqual({
      name: "relay",
      base_url: "https://r.example.com",
    });
  });

  it("saves the trimmed editor text and the shared meta", () => {
    expect(toHermesSettingsConfig(values)).toBe('{"name":"relay"}');
    const initialData = { meta: { endpointAutoSelect: false } };
    expect(
      toHermesProviderMeta(initialData, hermesFormFromProvider(initialData)),
    ).toMatchObject({ endpointAutoSelect: false, apiFormat: undefined });
  });

  it("ids presets by app and index", () => {
    expect(HERMES_PRESET_ENTRIES).toHaveLength(hermesProviderPresets.length);
    expect(hermesPresetEntries(hermesProviderPresets.slice(0, 2))).toEqual([
      { id: "hermes-0", preset: hermesProviderPresets[0] },
      { id: "hermes-1", preset: hermesProviderPresets[1] },
    ]);
  });
});
