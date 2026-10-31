import { describe, expect, it } from "vitest";
import i18n from "i18next";
import {
  applyCatalogOverlay,
  applyCatalogPromotions,
} from "@/config/presetCatalog";
import { providerPresets } from "@/config/claudeProviderPresets";

type P = {
  name: string;
  websiteUrl?: string;
  isPartner?: boolean;
  settingsConfig?: object;
  evil?: string;
};

const fixture = (): P[] => [
  { name: "A", websiteUrl: "https://a.example", settingsConfig: {} },
  { name: "B", websiteUrl: "https://b.example", settingsConfig: {} },
];

describe("preset catalog overlay", () => {
  it("leaves the list untouched for the bundled (empty) overlay", () => {
    const presets = fixture();
    const before = presets.map((p) => ({ ...p }));
    expect(applyCatalogOverlay(presets, undefined)).toBe(false);
    expect(applyCatalogOverlay(presets, { patch: {}, add: [] })).toBe(false);
    expect(presets).toEqual(before);
  });

  it("patches whitelisted fields in place, hides and adds presets", () => {
    const presets = fixture();
    const a = presets[0];
    const changed = applyCatalogOverlay(presets, {
      patch: {
        A: { websiteUrl: "https://new.example", isPartner: true, evil: "x" },
        B: { hidden: true },
      },
      add: [
        { name: "C", settingsConfig: { env: {} } },
        { name: "A", settingsConfig: {} }, // duplicate name ignored
        { name: "NoConfig" }, // invalid preset ignored
      ],
    });
    expect(changed).toBe(true);
    expect(presets.map((p) => p.name)).toEqual(["A", "C"]);
    expect(presets[0]).toBe(a);
    expect(a.websiteUrl).toBe("https://new.example");
    expect(a.isPartner).toBe(true);
    expect(a.evil).toBeUndefined();
  });

  it("registers promotion copy per language", () => {
    applyCatalogPromotions({
      catalogTestPromo: { en: "10% off with code TEST" },
    });
    expect(
      i18n.getResource(
        "en",
        "translation",
        "providerForm.partnerPromotion.catalogTestPromo",
      ),
    ).toBe("10% off with code TEST");
  });

  it("does not change the compiled-in presets without a catalog", () => {
    // Registration alone (test mode never fetches) keeps today's list.
    expect(providerPresets.length).toBeGreaterThan(10);
    expect(providerPresets.every((p) => typeof p.name === "string")).toBe(true);
  });
});
