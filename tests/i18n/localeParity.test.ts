import { describe, it, expect } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

// 所有语言包必须与 en.json 拥有完全相同的 key 集合，且插值变量一致。
// 缺失的 key 会静默回退到其他语言（如 zh-TW -> zh），多余的 key 则是无人使用的死翻译。
const LOCALES: Record<string, unknown> = { ja, zh, "zh-TW": zhTW };

const flatten = (node: unknown, prefix = ""): Record<string, unknown> => {
  if (!node || typeof node !== "object" || Array.isArray(node)) {
    return { [prefix]: node };
  }
  return Object.entries(node as Record<string, unknown>).reduce<
    Record<string, unknown>
  >(
    (acc, [key, value]) => ({
      ...acc,
      ...flatten(value, prefix ? `${prefix}.${key}` : key),
    }),
    {},
  );
};

const placeholders = (value: unknown): string[] =>
  typeof value === "string"
    ? Array.from(value.matchAll(/\{\{\s*(\w+)\s*\}\}/g), (m) => m[1]).sort()
    : [];

const enFlat = flatten(en);
const enKeys = Object.keys(enFlat).sort();

describe("i18n locale parity with en", () => {
  for (const [locale, resource] of Object.entries(LOCALES)) {
    const flat = flatten(resource);
    const keys = Object.keys(flat);

    it(`${locale}: has no missing keys`, () => {
      expect(enKeys.filter((key) => !(key in flat))).toEqual([]);
    });

    it(`${locale}: has no extra keys`, () => {
      expect(keys.filter((key) => !(key in enFlat)).sort()).toEqual([]);
    });

    it(`${locale}: keeps the same interpolation variables`, () => {
      const mismatched = enKeys.filter(
        (key) =>
          key in flat &&
          placeholders(flat[key]).join(",") !==
            placeholders(enFlat[key]).join(","),
      );
      expect(mismatched).toEqual([]);
    });
  }

  it("zh-TW tool install strings use Traditional Chinese", () => {
    const flat = flatten(zhTW);
    expect(flat["settings.toolInstall"]).toBe("安裝");
    expect(flat["settings.toolUpdate"]).toBe("升級");
    expect(flat["settings.manualInstallCommands"]).toBe("手動安裝命令");
  });
});
