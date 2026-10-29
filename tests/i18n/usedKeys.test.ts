import fs from "node:fs";
import path from "node:path";
import { describe, it, expect } from "vitest";
import en from "@/i18n/locales/en.json";

// 扫描 src/ 中所有字面量 t("...") / i18n.t("...") / i18nKey="..." 调用，确保每个 key 都存在于 en.json。
// en.json 是 fallbackLng，与其他语言包的一致性由 localeParity.test.ts 保证。
// 动态拼接的 key（模板字符串含 ${}）无法静态校验，必须在 DYNAMIC_KEY_PREFIXES 中显式登记。
const SRC_DIR = path.resolve(__dirname, "../../src");

// 动态 key 模板 -> 其展开后所在的 en.json 对象前缀。新增动态 key 时需在此登记。
const DYNAMIC_KEY_PREFIXES: Record<string, string> = {
  "apps.${app}": "apps",
  "apps.${appId}": "apps",
  "apps.${sharedFeatureApp}": "apps",
  "profiles.createDescription.${scope}": "profiles.createDescription",
  "profiles.switcherTooltip.${scope}": "profiles.switcherTooltip",
  "providerForm.partnerPromotion.${partnerPromotionKey}":
    "providerForm.partnerPromotion",
  "settings.advanced.logConfig.levels.${level}":
    "settings.advanced.logConfig.levels",
  "usage.appFilter.${appType}": "usage.appFilter",
  "usage.appFilter.${type}": "usage.appFilter",
};

// i18next 复数后缀：t("x", { count }) 会解析到 x_one / x_other 等
const PLURAL_SUFFIXES = ["_zero", "_one", "_two", "_few", "_many", "_other"];

// t( 前不能是标识符字符或 "."（排除 foo.t( / format( 等），但允许 i18n.t( / i18next.t(
const LITERAL_T_CALL =
  /(?:(?<![\w$.])|(?<=\bi18n\.|\bi18next\.))t\(\s*(["'`])((?:(?!\1)[^\\\n$])+)\1/g;
const DYNAMIC_T_CALL =
  /(?:(?<![\w$.])|(?<=\bi18n\.|\bi18next\.))t\(\s*`([^`]*\$\{[^`]*)`/g;
const I18N_KEY_PROP = /\bi18nKey=\{?\s*(["'`])([^"'`$\n]+)\1/g;

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

const listSourceFiles = (dir: string): string[] =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx|js|jsx)$/.test(entry.name) && !/\.d\.ts$/.test(entry.name)
      ? [full]
      : [];
  });

interface Usage {
  key: string;
  file: string;
}

const collectUsages = () => {
  const literal: Usage[] = [];
  const dynamic: Usage[] = [];
  for (const file of listSourceFiles(SRC_DIR)) {
    const source = fs.readFileSync(file, "utf8");
    const rel = path.relative(SRC_DIR, file).split(path.sep).join("/");
    for (const m of source.matchAll(LITERAL_T_CALL)) {
      literal.push({ key: m[2], file: rel });
    }
    for (const m of source.matchAll(I18N_KEY_PROP)) {
      literal.push({ key: m[2], file: rel });
    }
    for (const m of source.matchAll(DYNAMIC_T_CALL)) {
      dynamic.push({ key: m[1], file: rel });
    }
  }
  return { literal, dynamic };
};

const enFlat = flatten(en);
const existsInEn = (key: string) =>
  typeof enFlat[key] === "string" ||
  PLURAL_SUFFIXES.some((suffix) => typeof enFlat[key + suffix] === "string");

describe("i18n keys used in src/ exist in en.json", () => {
  const { literal, dynamic } = collectUsages();

  it("finds t() usages to check", () => {
    // 防止正则失效导致测试空跑
    expect(literal.length).toBeGreaterThan(500);
  });

  it("every literal key resolves in en.json", () => {
    const missing = Array.from(
      new Set(
        literal
          .filter(({ key }) => !existsInEn(key))
          .map(({ key, file }) => `${key} (${file})`),
      ),
    ).sort();
    expect(missing).toEqual([]);
  });

  it("every dynamic key template is allowlisted", () => {
    const unlisted = Array.from(
      new Set(
        dynamic
          .filter(({ key }) => !(key in DYNAMIC_KEY_PREFIXES))
          .map(({ key, file }) => `${key} (${file})`),
      ),
    ).sort();
    expect(unlisted).toEqual([]);
  });

  it("every allowlisted dynamic prefix is a non-empty object in en.json", () => {
    const broken = Object.entries(DYNAMIC_KEY_PREFIXES)
      .filter(([, prefix]) => {
        const node = lookup(prefix);
        return (
          !node ||
          typeof node !== "object" ||
          Object.keys(node as object).length === 0
        );
      })
      .map(([template]) => template);
    expect(broken).toEqual([]);
  });

  it("allowlist has no stale dynamic templates", () => {
    const used = new Set(dynamic.map(({ key }) => key));
    expect(
      Object.keys(DYNAMIC_KEY_PREFIXES).filter((t) => !used.has(t)),
    ).toEqual([]);
  });
});
