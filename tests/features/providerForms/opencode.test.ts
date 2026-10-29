import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { OPENCODE_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { opencodeProviderFormSchema } from "@/features/providers/forms/opencode/schema";
import {
  isOmoCategory,
  opencodeFormFromProvider,
} from "@/features/providers/forms/opencode/fromProvider";
import {
  OPENCODE_PRESET_ENTRIES,
  opencodePresetEntries,
} from "@/features/providers/forms/opencode/presets";
import {
  omoOtherFieldsError,
  opencodeProviderKeyForSave,
  toOpenCodeProviderMeta,
  toOpenCodeSettingsConfig,
} from "@/features/providers/forms/opencode/toSettingsConfig";

const t = ((key: string, options?: { field?: string }) =>
  options?.field ? `t:${key}(${options.field})` : `t:${key}`) as TFunction;

const values = { name: "R", settingsConfig: ' {"npm":"x"} ' };
const emptyDraft = { omoAgents: {}, omoCategories: {}, omoOtherFieldsStr: "" };

describe("opencode form module (CCS-192)", () => {
  it("schema is the opencode branch of the union", () => {
    expect(
      opencodeProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "opencode",
      }).success,
    ).toBe(true);
  });

  describe("fromProvider", () => {
    it("starts a new provider from the default config", () => {
      const state = opencodeFormFromProvider(undefined);
      expect(state.defaultValues.settingsConfig).toBe(OPENCODE_DEFAULT_CONFIG);
      expect(state.initialOmoSettings).toBeUndefined();
    });

    it("seeds the OMO draft only for OMO providers", () => {
      const settingsConfig = { agents: { a: { model: "m" } } };
      expect(
        opencodeFormFromProvider({ category: "omo", settingsConfig })
          .initialOmoSettings,
      ).toBe(settingsConfig);
      expect(
        opencodeFormFromProvider({ category: "omo-slim", settingsConfig })
          .initialOmoSettings,
      ).toBe(settingsConfig);
      expect(
        opencodeFormFromProvider({ category: "custom", settingsConfig })
          .initialOmoSettings,
      ).toBeUndefined();
    });

    it("recognises both OMO categories", () => {
      expect(isOmoCategory("omo")).toBe(true);
      expect(isOmoCategory("omo-slim")).toBe(true);
      expect(isOmoCategory("aggregator")).toBe(false);
      expect(isOmoCategory(undefined)).toBe(false);
    });
  });

  describe("toSettingsConfig", () => {
    it("saves the editor text for regular providers", () => {
      expect(
        toOpenCodeSettingsConfig({
          values,
          category: "custom",
          omoDraft: { ...emptyDraft, omoAgents: { a: {} } },
        }),
      ).toBe('{"npm":"x"}');
    });

    it("builds OMO config from the draft and omits empty parts", () => {
      const draft = {
        omoAgents: { sisyphus: { model: "m" } },
        omoCategories: { quick: { model: "q" } },
        omoOtherFieldsStr: '{"disabled_hooks":["x"]}',
      };
      expect(
        JSON.parse(
          toOpenCodeSettingsConfig({
            values,
            category: "omo",
            omoDraft: draft,
          }),
        ),
      ).toEqual({
        agents: { sisyphus: { model: "m" } },
        categories: { quick: { model: "q" } },
        otherFields: { disabled_hooks: ["x"] },
      });
      // OMO Slim 没有 categories
      expect(
        JSON.parse(
          toOpenCodeSettingsConfig({
            values,
            category: "omo-slim",
            omoDraft: draft,
          }),
        ),
      ).not.toHaveProperty("categories");
      expect(
        toOpenCodeSettingsConfig({
          values,
          category: "omo",
          omoDraft: emptyDraft,
        }),
      ).toBe("{}");
    });

    it("rejects OMO other fields that are not a JSON object", () => {
      expect(omoOtherFieldsError("  ", t)).toBeNull();
      expect(omoOtherFieldsError('{"a":1}', t)).toBeNull();
      expect(omoOtherFieldsError("[1]", t)).toBe(
        "t:omo.jsonMustBeObject(t:omo.otherFields)",
      );
      expect(omoOtherFieldsError("{", t)).toBe("t:omo.invalidJson");
    });

    it("generates OMO keys on create and keeps them on edit", () => {
      const randomId = () => "0123456789abcdef";
      expect(
        opencodeProviderKeyForSave({
          category: "omo",
          isEditMode: false,
          providerKey: "",
          randomId,
        }),
      ).toBe("omo-01234567");
      expect(
        opencodeProviderKeyForSave({
          category: "omo-slim",
          isEditMode: false,
          providerKey: "",
          randomId,
        }),
      ).toBe("omo-slim-01234567");
      expect(
        opencodeProviderKeyForSave({
          category: "omo",
          isEditMode: true,
          providerKey: "",
          randomId,
        }),
      ).toBeUndefined();
      expect(
        opencodeProviderKeyForSave({
          category: "custom",
          isEditMode: true,
          providerKey: "relay",
        }),
      ).toBe("relay");
    });

    it("saves the shared meta", () => {
      const initialData = { meta: { endpointAutoSelect: false } };
      expect(
        toOpenCodeProviderMeta(
          initialData,
          opencodeFormFromProvider(initialData),
        ),
      ).toMatchObject({ endpointAutoSelect: false, apiKeyField: undefined });
    });
  });

  it("ids presets by app and index", () => {
    expect(OPENCODE_PRESET_ENTRIES).toHaveLength(
      opencodeProviderPresets.length,
    );
    expect(opencodePresetEntries(opencodeProviderPresets.slice(0, 1))).toEqual([
      { id: "opencode-0", preset: opencodeProviderPresets[0] },
    ]);
  });
});
