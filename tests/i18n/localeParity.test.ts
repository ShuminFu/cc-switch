import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

type Messages = { [key: string]: unknown };

const locales: Record<string, Messages> = {
  en,
  zh,
  "zh-TW": zhTW,
  ja,
};

/** Flatten nested messages into a map of dotted key -> leaf value. */
function flatten(
  obj: Messages,
  prefix = "",
  out: Map<string, unknown> = new Map(),
): Map<string, unknown> {
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === "object" && !Array.isArray(value)) {
      flatten(value as Messages, path, out);
    } else {
      out.set(path, value);
    }
  }
  return out;
}

/** Sorted interpolation variable names, e.g. "{{count}} of {{total}}" -> "count,total". */
function placeholders(value: unknown): string {
  if (typeof value !== "string") return "";
  return [...value.matchAll(/\{\{\s*([\w.]+)\s*(?:,[^}]*)?\}\}/g)]
    .map((m) => m[1])
    .sort()
    .join(",");
}

const flattened = Object.fromEntries(
  Object.entries(locales).map(([name, messages]) => [name, flatten(messages)]),
);
const reference = flattened.en;

describe("i18n locale files", () => {
  for (const [name, keys] of Object.entries(flattened)) {
    if (name === "en") continue;

    it(`${name} has exactly the same keys as en`, () => {
      const missing = [...reference.keys()].filter((k) => !keys.has(k));
      const extra = [...keys.keys()].filter((k) => !reference.has(k));
      expect({ missing, extra }).toEqual({ missing: [], extra: [] });
    });

    it(`${name} uses the same interpolation placeholders as en`, () => {
      const mismatched = [...reference.entries()]
        .filter(([k]) => keys.has(k))
        .filter(([k, v]) => placeholders(v) !== placeholders(keys.get(k)))
        .map(
          ([k, v]) =>
            `${k}: en={${placeholders(v)}} ${name}={${placeholders(keys.get(k))}}`,
        );
      expect(mismatched).toEqual([]);
    });
  }
});
