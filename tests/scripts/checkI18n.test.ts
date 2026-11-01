import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const SCRIPT = path.resolve(__dirname, "../../scripts/check-i18n.mjs");

const dirs: string[] = [];

function run(locales: Record<string, unknown>) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "check-i18n-"));
  dirs.push(dir);
  for (const [name, content] of Object.entries(locales)) {
    fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify(content));
  }
  const res = spawnSync(process.execPath, [SCRIPT, dir], { encoding: "utf8" });
  return { code: res.status, out: `${res.stdout}${res.stderr}` };
}

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("scripts/check-i18n.mjs", () => {
  it("passes when key sets and placeholders match", () => {
    const res = run({
      en: { a: { b: "Hello {{name}}" }, items_one: "{{count}} item" },
      zh: { a: { b: "你好 {{ name }}" }, items_other: "{{count}} 项" },
    });
    expect(res.code).toBe(0);
    expect(res.out).toContain("2 locales OK");
  });

  it("fails on a key added to en.json only", () => {
    const res = run({ en: { a: "A", b: "B" }, ja: { a: "A" } });
    expect(res.code).toBe(1);
    expect(res.out).toContain("ja.json: 1 missing keys");
    expect(res.out).toContain("- b");
  });

  it("fails on extra keys in a non-reference locale", () => {
    const res = run({ en: { a: "A" }, "zh-TW": { a: "A", stale: "x" } });
    expect(res.code).toBe(1);
    expect(res.out).toContain("- stale");
  });

  it("fails on placeholder mismatch", () => {
    const res = run({
      en: { msg: "Failed: {{error}}" },
      zh: { msg: "失败：{{err}}" },
    });
    expect(res.code).toBe(1);
    expect(res.out).toContain("placeholder mismatches");
    expect(res.out).toContain("msg: en {error} vs {err}");
  });

  it("fails on value type mismatch", () => {
    const res = run({ en: { a: { b: "x" } }, zh: { a: "x" } });
    expect(res.code).toBe(1);
  });
});
