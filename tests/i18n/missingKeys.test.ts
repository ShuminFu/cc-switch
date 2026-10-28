import { describe, it, expect } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

// 这些 key 在代码中通过 t() 使用，但曾在所有语言包中缺失，导致英文/日文用户看到中文 defaultValue。
// 每个 key 附带代码调用处传入的插值变量，翻译中必须保留。
const KEYS: Record<string, string[]> = {
  "providerForm.requiredFields": [],
  "codexConfig.noCommonConfigToApply": [],
  "providerForm.providerKeyStatusLoading": [],
  "usage.unpriced": [],
  "failover.tooltip.takeoverRequired": ["app"],
  "claudeDesktop.route.stopBlockedByTakeover": [],
  "proxy.server.stopped": [],
  "proxy.server.stopFailed": ["detail"],
  "notifications.proxyReasonClaudeDesktop": [],
  "provider.duplicateLiveIdsLoadFailed": [],
};

const LOCALES: Record<string, unknown> = { en, ja, zh, "zh-TW": zhTW };

const lookup = (resource: unknown, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      resource,
    );

const placeholders = (value: string): string[] =>
  Array.from(value.matchAll(/\{\{\s*(\w+)\s*\}\}/g), (m) => m[1]).sort();

describe("i18n keys used in code", () => {
  for (const [locale, resource] of Object.entries(LOCALES)) {
    for (const [key, vars] of Object.entries(KEYS)) {
      it(`${locale}: ${key}`, () => {
        const value = lookup(resource, key);
        expect(typeof value).toBe("string");
        expect((value as string).trim()).not.toBe("");
        expect(placeholders(value as string)).toEqual([...vars].sort());
      });
    }
  }

  it("en/ja translations are not left in Chinese", () => {
    for (const locale of ["en", "ja"]) {
      for (const key of Object.keys(KEYS)) {
        expect(lookup(LOCALES[locale], key)).not.toBe(lookup(zh, key));
      }
    }
  });
});
