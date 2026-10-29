import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";
import i18next from "i18next";
import { LOCALE_LOADERS, lazyLocaleBackend } from "@/i18n";
import ja from "@/i18n/locales/ja.json";
import en from "@/i18n/locales/en.json";

const ROOT = path.resolve(__dirname, "../..");

describe("lazy locale loading", () => {
  it("does not statically import locale JSON into the entry bundle", () => {
    const source = readFileSync(path.join(ROOT, "src/i18n/index.ts"), "utf8");
    expect(source).not.toMatch(/^import\s[^;]*locales\/[^"]+\.json/m);
    expect(Object.keys(LOCALE_LOADERS).sort()).toEqual(
      ["en", "ja", "zh", "zh-TW"].sort(),
    );
  });

  it("loads only the active language and its fallback on demand", async () => {
    const instance = i18next.createInstance();
    await instance.use(lazyLocaleBackend).init({
      lng: "ja",
      fallbackLng: "en",
      supportedLngs: Object.keys(LOCALE_LOADERS),
      interpolation: { escapeValue: false },
    });

    expect(instance.t("common.save")).toBe(ja.common.save);
    expect(instance.hasResourceBundle("ja", "translation")).toBe(true);
    expect(instance.hasResourceBundle("en", "translation")).toBe(true);
    expect(instance.hasResourceBundle("zh", "translation")).toBe(false);
    expect(instance.hasResourceBundle("zh-TW", "translation")).toBe(false);

    await instance.changeLanguage("en");
    expect(instance.t("common.save")).toBe(en.common.save);
    expect(instance.hasResourceBundle("zh", "translation")).toBe(false);
  });
});
