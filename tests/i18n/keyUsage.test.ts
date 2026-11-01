import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";

type LocaleTree = { [key: string]: string | LocaleTree };

const flattenKeys = (tree: LocaleTree, prefix = ""): string[] =>
  Object.entries(tree).flatMap(([key, value]) => {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    return value !== null && typeof value === "object"
      ? flattenKeys(value, fullKey)
      : [fullKey];
  });

const listSourceFiles = (dir: string): string[] =>
  readdirSync(dir).flatMap((name) => {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx)$/.test(name) ? [full] : [];
  });

describe("i18n key usage", () => {
  it('every literal t("key") call in src/ has an entry in en.json', () => {
    const keys = new Set(flattenKeys(en as LocaleTree));
    const hasKey = (key: string) =>
      keys.has(key) || [...keys].some((k) => k.startsWith(`${key}_`)); // plural forms

    const srcDir = path.resolve(__dirname, "../../src");
    const pattern = /\bt\(\s*["'`]([A-Za-z0-9_.-]+)["'`]/g;
    const missing: string[] = [];

    for (const file of listSourceFiles(srcDir)) {
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(pattern)) {
        if (!hasKey(match[1])) {
          missing.push(`${path.relative(srcDir, file)}: ${match[1]}`);
        }
      }
    }

    expect(missing).toEqual([]);
  });
});
