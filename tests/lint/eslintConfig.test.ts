import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";
import { ESLint } from "eslint";
import { APP_IDS } from "../../eslint-rules/index.js";
import { ALL_APP_IDS } from "@/entities/app/registry";

// 确保 eslint.config.js 会对新文件中的 Hooks 规则违规报错（CI 中 pnpm lint 因此失败），
// 并且基线文件 eslint-suppressions.json 不会屏蔽任何 rules-of-hooks 违规。
const ROOT = path.resolve(__dirname, "../..");

// 基线中 app-id 字面量比较的总数上限：只能下降，修掉后请同步调低。
const APP_LITERAL_BUDGET = 173;
// 现有直接 import invoke 的文件；不允许新增。
const RAW_INVOKE_BASELINE = [
  "src/components/DatabaseUpgrade.tsx",
  "src/components/theme-provider.tsx",
  "src/hooks/useProxyConfig.ts",
  "src/hooks/useProxyStatus.ts",
  "src/lib/clipboard.ts",
  "src/main.tsx",
];

type Suppressions = Record<string, Record<string, { count: number }>>;
const readSuppressions = () =>
  JSON.parse(
    readFileSync(path.join(ROOT, "eslint-suppressions.json"), "utf8"),
  ) as Suppressions;

const lint = async (code: string, filePath: string) => {
  const eslint = new ESLint({ cwd: ROOT });
  const [result] = await eslint.lintText(code, {
    filePath: path.join(ROOT, filePath),
  });
  return result;
};

describe("eslint config", { timeout: 30_000 }, () => {
  it("reports a conditional hook call in a new src file as an error", async () => {
    const result = await lint(
      [
        'import { useState } from "react";',
        "export function Bad({ on }: { on: boolean }) {",
        "  if (on) {",
        "    const [v] = useState(0);",
        "    return <span>{v}</span>;",
        "  }",
        "  return null;",
        "}",
        "",
      ].join("\n"),
      "src/components/__lintFixture__.tsx",
    );

    const hookErrors = result.messages.filter(
      (m) => m.ruleId === "react-hooks/rules-of-hooks",
    );
    expect(hookErrors).toHaveLength(1);
    expect(hookErrors[0].severity).toBe(2);
  });

  it("lints test files too", async () => {
    const result = await lint(
      [
        'import { useEffect } from "react";',
        "export function helper() {",
        "  useEffect(() => {});",
        "}",
        "",
      ].join("\n"),
      "tests/__lintFixture__.test.tsx",
    );

    expect(result.messages.map((m) => m.ruleId)).toContain(
      "react-hooks/rules-of-hooks",
    );
  });

  it("accepts valid hook usage", async () => {
    const result = await lint(
      [
        'import { useState } from "react";',
        "export function Good() {",
        "  const [v] = useState(0);",
        "  return <span>{v}</span>;",
        "}",
        "",
      ].join("\n"),
      "src/components/__lintFixture__.tsx",
    );

    expect(result.messages).toEqual([]);
  });

  it("baseline never suppresses rules-of-hooks", () => {
    const suppressions = JSON.parse(
      readFileSync(path.join(ROOT, "eslint-suppressions.json"), "utf8"),
    ) as Record<string, Record<string, unknown>>;

    for (const [file, rules] of Object.entries(suppressions)) {
      expect(
        Object.keys(rules),
        `${file} suppresses rules-of-hooks`,
      ).not.toContain("react-hooks/rules-of-hooks");
    }
  });

  it("reports a raw invoke import in a new src file as an error", async () => {
    const result = await lint(
      [
        'import { invoke } from "@tauri-apps/api/core";',
        'export const load = () => invoke("get_settings");',
        "",
      ].join("\n"),
      "src/components/__lintFixture__.ts",
    );

    const errors = result.messages.filter(
      (m) => m.ruleId === "no-restricted-imports",
    );
    expect(errors).toHaveLength(1);
    expect(errors[0].severity).toBe(2);
  });

  it("reports a namespace import that reaches invoke", async () => {
    const result = await lint(
      [
        'import * as core from "@tauri-apps/api/core";',
        'export const load = () => core.invoke("get_settings");',
        "",
      ].join("\n"),
      "src/hooks/__lintFixture__.ts",
    );

    expect(result.messages.map((m) => m.ruleId)).toContain(
      "no-restricted-imports",
    );
  });

  it("reports invoke reached through the package root or a dynamic import", async () => {
    const result = await lint(
      [
        'import { core } from "@tauri-apps/api";',
        'export const a = () => core.invoke("get_settings");',
        "export const b = async () =>",
        '  (await import("@tauri-apps/api/core")).invoke("get_settings");',
        "",
      ].join("\n"),
      "src/hooks/__lintFixture__.ts",
    );

    const errors = result.messages.filter(
      (m) =>
        m.ruleId === "no-restricted-imports" ||
        m.ruleId === "no-restricted-syntax",
    );
    expect(errors.map((m) => m.ruleId).sort()).toEqual([
      "no-restricted-imports",
      "no-restricted-syntax",
    ]);
    expect(errors.every((m) => m.severity === 2)).toBe(true);
  });

  it("allows invoke inside the API layer and other core imports elsewhere", async () => {
    const apiLayer = await lint(
      [
        'import { invoke } from "@tauri-apps/api/core";',
        'export const load = () => invoke("get_settings");',
        "",
      ].join("\n"),
      "src/lib/api/__lintFixture__.ts",
    );
    expect(apiLayer.messages).toEqual([]);

    const otherImport = await lint(
      [
        'import { convertFileSrc } from "@tauri-apps/api/core";',
        "export const src = convertFileSrc;",
        "",
      ].join("\n"),
      "src/components/__lintFixture__.ts",
    );
    expect(otherImport.messages).toEqual([]);
  });

  it("reports app-id literal comparisons in a new src file", async () => {
    const result = await lint(
      [
        'import type { AppId } from "@/lib/api/types";',
        "export function f(appId: AppId) {",
        '  if (appId === "claude") return 1;',
        '  if ("hermes" !== appId) return 2;',
        "  switch (appId) {",
        '    case "codex":',
        "      return 3;",
        "  }",
        "  return 0;",
        "}",
        "",
      ].join("\n"),
      "src/components/__lintFixture__.ts",
    );

    const errors = result.messages.filter(
      (m) => m.ruleId === "cc-switch/no-app-literal-comparison",
    );
    expect(errors).toHaveLength(3);
    expect(errors.every((m) => m.severity === 2)).toBe(true);
  });

  it("accepts registry lookups and non-app literals", async () => {
    const result = await lint(
      [
        'import { appCaps } from "@/entities/app/registry";',
        'import type { AppId } from "@/lib/api/types";',
        "export function f(appId: AppId, category: string) {",
        "  if (appCaps(appId).additive) return 1;",
        '  if (category === "official") return 2;',
        "  return 0;",
        "}",
        "",
      ].join("\n"),
      "src/components/__lintFixture__.ts",
    );

    expect(result.messages).toEqual([]);
  });

  it("lint rule app ids match the app registry", () => {
    expect([...APP_IDS].sort()).toEqual([...ALL_APP_IDS].sort());
  });

  it("keeps the app-literal baseline within budget", () => {
    const total = Object.values(readSuppressions()).reduce(
      (sum, rules) =>
        sum + (rules["cc-switch/no-app-literal-comparison"]?.count ?? 0),
      0,
    );
    expect(total).toBeLessThanOrEqual(APP_LITERAL_BUDGET);
    expect(APP_LITERAL_BUDGET).toBeLessThanOrEqual(200);
  });

  it("never adds files to the raw invoke baseline", () => {
    const files = Object.entries(readSuppressions())
      .filter(
        ([, rules]) =>
          "no-restricted-imports" in rules || "no-restricted-syntax" in rules,
      )
      .map(([file]) => file);
    for (const file of files) {
      expect(RAW_INVOKE_BASELINE, `${file} newly imports invoke`).toContain(
        file,
      );
    }
  });
});
