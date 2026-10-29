import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, expectTypeOf, it } from "vitest";
import type { TFunction } from "i18next";
import { ALL_APP_IDS } from "@/entities/app/registry";
import {
  activePresetFrom,
  presetBaseFormValues,
  providerFormResolver,
  providerFormSchema,
  providerFormSchemas,
  type BasePreset,
} from "@/features/providers/forms";
import { PROVIDER_FORM_EDITORS } from "@/components/providers/forms/ProviderForm";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import type { OpenClawProviderPreset } from "@/config/openclawProviderPresets";
import type { HermesProviderPreset } from "@/config/hermesProviderPresets";

const ROOT = path.resolve(__dirname, "../..");
const lineCount = (file: string) =>
  readFileSync(path.join(ROOT, file), "utf8").trimEnd().split("\n").length;

const validValues = {
  name: "Relay",
  websiteUrl: "",
  notes: "",
  settingsConfig: "{}",
  icon: "",
  iconColor: "",
};

const resolverOptions = {
  fields: {},
  shouldUseNativeValidation: false,
} as Parameters<ReturnType<typeof providerFormResolver>>[2];

describe("provider form schema (CCS-190)", () => {
  it("has one discriminated-union variant per registered app", () => {
    expect(Object.keys(providerFormSchemas).sort()).toEqual(
      [...ALL_APP_IDS].sort(),
    );
    for (const appId of ALL_APP_IDS) {
      expect(
        providerFormSchema.safeParse({ ...validValues, appId }).success,
      ).toBe(true);
    }
    expect(
      providerFormSchema.safeParse({ ...validValues, appId: "unknown" })
        .success,
    ).toBe(false);
  });

  it("validates through the union and keeps appId out of the values", async () => {
    const resolver = providerFormResolver("codex");
    const ok = await resolver(validValues, undefined, resolverOptions);
    expect(ok.errors).toEqual({});
    expect(ok.values).toEqual(validValues);
    expect(ok.values).not.toHaveProperty("appId");

    const bad = await resolver(
      { ...validValues, settingsConfig: '{"a": ' },
      undefined,
      resolverOptions,
    );
    expect(bad.values).toEqual({});
    expect(bad.errors.settingsConfig?.message).toMatch(/JSON/);
  });
});

describe("BasePreset (CCS-190)", () => {
  it("is the shared base of every app's preset type", () => {
    expectTypeOf<ProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<ClaudeDesktopProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<CodexProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<GeminiProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<OpenCodeProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<OpenClawProviderPreset>().toExtend<BasePreset>();
    expectTypeOf<HermesProviderPreset>().toExtend<BasePreset>();
  });

  it("derives the reset values and the active preset", () => {
    const t = ((key: string) => `t:${key}`) as unknown as TFunction;
    const preset: BasePreset = {
      name: "Relay",
      nameKey: "providerForm.categoryOfficial",
      websiteUrl: "https://relay.example.com",
      category: "aggregator",
      isPartner: true,
      partnerPromotionKey: "promo",
      icon: "relay",
    };

    expect(presetBaseFormValues(preset, "{}", t)).toEqual({
      name: "t:providerForm.categoryOfficial",
      websiteUrl: "https://relay.example.com",
      settingsConfig: "{}",
      icon: "relay",
      iconColor: "",
    });
    expect(presetBaseFormValues(preset, "{}", t, "OMO").name).toBe("OMO");
    expect(activePresetFrom("codex-3", preset)).toEqual({
      id: "codex-3",
      category: "aggregator",
      isPartner: true,
      partnerPromotionKey: "promo",
    });
  });
});

describe("ProviderForm shell (CCS-190)", () => {
  it("registers an editor for every app", () => {
    expect(Object.keys(PROVIDER_FORM_EDITORS).sort()).toEqual(
      [...ALL_APP_IDS].sort(),
    );
  });

  it("keeps ProviderForm.tsx and the shell at 300 lines or fewer", () => {
    expect(
      lineCount("src/components/providers/forms/ProviderForm.tsx"),
    ).toBeLessThanOrEqual(300);
    expect(
      lineCount("src/features/providers/forms/ProviderFormShell.tsx"),
    ).toBeLessThanOrEqual(300);
  });
});
