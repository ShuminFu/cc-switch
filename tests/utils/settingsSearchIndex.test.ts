import { describe, expect, it } from "vitest";
import {
  SETTINGS_SEARCH_INDEX,
  searchSettings,
  settingsEntryTitle,
} from "@/components/settings/settingsSearchIndex";
import en from "@/i18n/locales/en.json";

const lookup = (key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      en,
    );

/** English translate function backed by the real locale file. */
const t = (key: string, options?: Record<string, unknown>) => {
  const value = lookup(key);
  return typeof value === "string"
    ? value
    : ((options?.defaultValue as string) ?? key);
};

describe("settings search index (FE-UI-38)", () => {
  it("finds the Claude onboarding toggle on the General tab", () => {
    const [first] = searchSettings("onboarding", t);
    expect(first.id).toBe("skipClaudeOnboarding");
    expect(first.tab).toBe("general");
  });

  it("matches keywords and localized titles, all words required", () => {
    expect(searchSettings("webdav", t).map((e) => e.id)).toEqual(["cloudSync"]);
    expect(searchSettings("media heuristic", t)[0].section).toBe("rectifier");
    expect(searchSettings("webdav onboarding", t)).toEqual([]);
    expect(searchSettings("   ", t)).toEqual([]);
  });

  it("uses title keys that exist in the English locale", () => {
    for (const entry of SETTINGS_SEARCH_INDEX) {
      expect(typeof lookup(entry.titleKey), entry.titleKey).toBe("string");
      expect(settingsEntryTitle(entry, t)).not.toBe(entry.titleKey);
    }
  });

  it("has unique ids", () => {
    const ids = SETTINGS_SEARCH_INDEX.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
