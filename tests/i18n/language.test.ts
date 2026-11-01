// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import i18next from "i18next";
import {
  getInitialLanguage,
  mapLocaleToLanguage,
  syncDocumentLanguage,
} from "@/i18n/language";

describe("getInitialLanguage", () => {
  afterEach(() => {
    window.localStorage.removeItem("language");
    vi.restoreAllMocks();
  });

  it("maps supported locales", () => {
    expect(getInitialLanguage("ja-JP")).toBe("ja");
    expect(getInitialLanguage("en-US")).toBe("en");
    expect(getInitialLanguage("zh")).toBe("zh");
    expect(getInitialLanguage("zh-CN")).toBe("zh");
    expect(getInitialLanguage("zh-Hans-CN")).toBe("zh");
  });

  it("maps Traditional Chinese variants to zh-TW", () => {
    for (const locale of ["zh-TW", "zh-HK", "zh-MO", "zh-Hant", "zh_TW"]) {
      expect(getInitialLanguage(locale)).toBe("zh-TW");
    }
  });

  it("falls back to English for unknown locales", () => {
    for (const locale of ["de-DE", "fr-FR", "ko-KR", "es-ES", "pt-BR", ""]) {
      expect(getInitialLanguage(locale)).toBe("en");
    }
    expect(mapLocaleToLanguage(undefined)).toBe("en");
  });

  it("prefers the stored language over the system locale", () => {
    window.localStorage.setItem("language", "ja");
    expect(getInitialLanguage()).toBe("ja");
  });

  it("uses navigator.language when nothing is stored", () => {
    vi.spyOn(window.navigator, "language", "get").mockReturnValue("de-DE");
    expect(getInitialLanguage()).toBe("en");
    vi.spyOn(window.navigator, "language", "get").mockReturnValue("zh-HK");
    expect(getInitialLanguage()).toBe("zh-TW");
  });
});

describe("syncDocumentLanguage", () => {
  it("keeps <html lang> in sync with the active i18n language", async () => {
    const instance = i18next.createInstance();
    instance.on("languageChanged", syncDocumentLanguage);
    await instance.init({ lng: "ja", resources: {} });
    expect(document.documentElement.lang).toBe("ja");
    await instance.changeLanguage("zh-TW");
    expect(document.documentElement.lang).toBe("zh-TW");
    expect(document.documentElement.lang).toBe(instance.language);
  });
});

describe("index.html", () => {
  it("ships a neutral default <html lang> that i18n replaces at runtime", () => {
    const html = readFileSync(
      path.resolve(__dirname, "../../src/index.html"),
      "utf8",
    );
    expect(html).toMatch(/<html lang="en">/);
  });
});
