import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";
import { ESLint } from "eslint";

// 确保 eslint.config.js 会对新文件中的 Hooks 规则违规报错（CI 中 pnpm lint 因此失败），
// 并且基线文件 eslint-suppressions.json 不会屏蔽任何 rules-of-hooks 违规。
const ROOT = path.resolve(__dirname, "../..");

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
});
