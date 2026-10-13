import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";

type Messages = { [key: string]: unknown };

const SRC_DIR = path.resolve(__dirname, "../../src");

/** i18next plural / ordinal suffixes that may stand in for a base key. */
const PLURAL_SUFFIXES = ["zero", "one", "two", "few", "many", "other"];

function flattenKeys(obj: Messages, prefix = "", out = new Set<string>()) {
  for (const [key, value] of Object.entries(obj)) {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === "object" && !Array.isArray(value)) {
      flattenKeys(value as Messages, fullKey, out);
    } else {
      out.add(fullKey);
    }
  }
  return out;
}

function listSourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)
      ? [full]
      : [];
  });
}

/**
 * Matches literal keys passed as the first argument to `t(...)`,
 * `i18n.t(...)`, `i18next.t(...)`. Template literals containing `${` are
 * dynamic and therefore skipped (the character class excludes `$`).
 */
const T_CALL = /\bt\(\s*(["'`])([A-Za-z0-9_.-]+)\1/g;

function collectUsedKeys() {
  const used: Array<{ key: string; file: string }> = [];
  for (const file of listSourceFiles(SRC_DIR)) {
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(T_CALL)) {
      used.push({ key: match[2], file: path.relative(SRC_DIR, file) });
    }
  }
  return used;
}

describe("i18n keys used in src", () => {
  const enKeys = flattenKeys(en as Messages);
  const exists = (key: string) =>
    enKeys.has(key) ||
    PLURAL_SUFFIXES.some((suffix) => enKeys.has(`${key}_${suffix}`));

  it("finds a meaningful number of literal t() calls", () => {
    // Guards against the scanner silently matching nothing.
    expect(collectUsedKeys().length).toBeGreaterThan(500);
  });

  it('every literal t("key") exists in en.json', () => {
    const missing = collectUsedKeys()
      .filter(({ key }) => !exists(key))
      .map(({ key, file }) => `${key} (${file})`);
    expect([...new Set(missing)].sort()).toEqual([]);
  });
});
