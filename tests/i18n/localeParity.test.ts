import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

type LocaleTree = { [key: string]: string | LocaleTree };

const flatten = (tree: LocaleTree, prefix = ""): Map<string, string> => {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === "object") {
      for (const [k, v] of flatten(value, path)) out.set(k, v);
    } else {
      out.set(path, String(value));
    }
  }
  return out;
};

const placeholders = (text: string): string[] =>
  Array.from(text.matchAll(/\{\{\s*([^}\s,]+)[^}]*\}\}/g), (m) => m[1]).sort();

const reference = flatten(en as LocaleTree);
const others = {
  zh: flatten(zh as LocaleTree),
  "zh-TW": flatten(zhTW as LocaleTree),
  ja: flatten(ja as LocaleTree),
};

describe("locale parity", () => {
  it.each(Object.entries(others))(
    "%s has exactly the same keys as en.json",
    (_locale, keys) => {
      const missing = [...reference.keys()].filter((k) => !keys.has(k));
      const extra = [...keys.keys()].filter((k) => !reference.has(k));
      expect({ missing, extra }).toEqual({ missing: [], extra: [] });
    },
  );

  it.each(Object.entries(others))(
    "%s uses the same {{placeholders}} as en.json",
    (_locale, keys) => {
      const mismatched: string[] = [];
      for (const [key, enText] of reference) {
        const text = keys.get(key);
        if (text === undefined) continue;
        const want = placeholders(enText);
        const got = placeholders(text);
        if (want.join("|") !== got.join("|")) {
          mismatched.push(`${key}: en=[${want}] got=[${got}]`);
        }
      }
      expect(mismatched).toEqual([]);
    },
  );
});
