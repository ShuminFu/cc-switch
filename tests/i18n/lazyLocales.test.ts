import i18next, { type ResourceKey } from "i18next";
import { describe, expect, it } from "vitest";
import { lazyLocaleBackend } from "@/i18n";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";

const read = (language: string) =>
  new Promise<ResourceKey | null | boolean | undefined>((resolve, reject) => {
    lazyLocaleBackend.read(language, "translation", (error, data) =>
      error ? reject(error) : resolve(data),
    );
  });

describe("lazy locale loading", () => {
  it("resolves bundled and lazily imported locales", async () => {
    expect(await read("en")).toEqual(en);
    expect(await read("ja")).toEqual(ja);
    expect(await read("de")).toEqual({});
  });

  it("loads only the selected locale when the language changes", async () => {
    const instance = i18next.createInstance();
    await instance.use(lazyLocaleBackend).init({
      resources: { en: { translation: en } },
      partialBundledLanguages: true,
      lng: "en",
      fallbackLng: "en",
      interpolation: { escapeValue: false },
    });

    expect(instance.hasResourceBundle("zh", "translation")).toBe(false);
    expect(instance.hasResourceBundle("ja", "translation")).toBe(false);

    await instance.changeLanguage("zh");

    expect(instance.hasResourceBundle("zh", "translation")).toBe(true);
    expect(instance.hasResourceBundle("ja", "translation")).toBe(false);
    expect(instance.t("settings.title")).toBe(zh.settings.title);
  });
});
