import { describe, expect, it } from "vitest";
import { GEMINI_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import { geminiProviderFormSchema } from "@/features/providers/forms/gemini/schema";
import {
  geminiEnvToString,
  geminiFormFromProvider,
} from "@/features/providers/forms/gemini/fromProvider";
import {
  geminiEnvToObject,
  toGeminiAppMeta,
  toGeminiSettingsConfig,
} from "@/features/providers/forms/gemini/toSettingsConfig";
import {
  geminiPresetEntries,
  geminiPresetLoader,
} from "@/features/providers/forms/gemini/presets";

describe("gemini form module (CCS-191)", () => {
  it("schema is the gemini branch of the union", () => {
    expect(
      geminiProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "gemini",
      }).success,
    ).toBe(true);
  });

  describe("fromProvider", () => {
    it("uses the default settings for a new provider", () => {
      const state = geminiFormFromProvider(undefined);
      expect(state.defaultValues.settingsConfig).toBe(GEMINI_DEFAULT_CONFIG);
      expect(state.endpointAutoSelect).toBe(true);
    });

    it("keeps the saved settings and pricing", () => {
      const state = geminiFormFromProvider({
        name: "G",
        settingsConfig: { env: { GEMINI_API_KEY: "k" }, config: {} },
        meta: { pricingModelSource: "request" },
      });
      expect(JSON.parse(state.defaultValues.settingsConfig)).toEqual({
        env: { GEMINI_API_KEY: "k" },
        config: {},
      });
      expect(state.pricing).toMatchObject({
        enabled: true,
        pricingModelSource: "request",
      });
    });

    it("renders the env with known keys first and skips empty ones", () => {
      expect(
        geminiEnvToString({
          CUSTOM: "1",
          GEMINI_MODEL: "gemini-3",
          GEMINI_API_KEY: "",
          GOOGLE_GEMINI_BASE_URL: "https://g.example",
          NOT_A_STRING: 2,
        }),
      ).toBe(
        "GOOGLE_GEMINI_BASE_URL=https://g.example\nGEMINI_MODEL=gemini-3\nCUSTOM=1\nGEMINI_API_KEY=",
      );
    });
  });

  describe("toSettingsConfig", () => {
    it("parses the .env text, ignoring comments and blank lines", () => {
      expect(
        geminiEnvToObject("# note\n\nGEMINI_API_KEY = k=v \n=bad\nNOEQ\n"),
      ).toEqual({ GEMINI_API_KEY: "k=v" });
    });

    it("saves env and config together", () => {
      expect(
        JSON.parse(
          toGeminiSettingsConfig({
            env: "GEMINI_API_KEY=k\nGEMINI_MODEL=m",
            config: { general: { a: 1 } },
          }),
        ),
      ).toEqual({
        env: { GEMINI_API_KEY: "k", GEMINI_MODEL: "m" },
        config: { general: { a: 1 } },
      });
    });

    it("only manages the common-config flag in meta", () => {
      const meta = toGeminiAppMeta({ commonConfigEnabled: true });
      expect(meta.commonConfigEnabled).toBe(true);
      expect(
        Object.entries(meta).filter(([key, value]) =>
          key === "commonConfigEnabled" ? false : value !== undefined,
        ),
      ).toEqual([]);
    });
  });

  it("loads the presets lazily", async () => {
    const presets = await geminiPresetLoader.load();
    expect(geminiPresetEntries(presets)[0].id).toBe("gemini-0");
  });
});
