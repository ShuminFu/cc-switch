import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { afterEach, describe, expect, it } from "vitest";
import {
  ALLOWLIST,
  checkUsageTokenNames,
  firstTokenLine,
} from "../../scripts/check-usage-token-names.mjs";

// CCS-243：cached_tokens / cache_creation_input_tokens 只能出现在 codec/ 与 usage/
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/check-usage-token-names.mjs");

const run = (args: string[] = []) =>
  spawnSync(process.execPath, [SCRIPT, ...args], {
    cwd: ROOT,
    encoding: "utf8",
  });

describe("checkUsageTokenNames", () => {
  const tmpDirs: string[] = [];
  afterEach(() => {
    for (const d of tmpDirs.splice(0)) rmSync(d, { recursive: true });
  });

  it("finds the first line that names a cache-token field", () => {
    expect(firstTokenLine("a\nlet x = u.cached_tokens;\n")).toBe(2);
    expect(firstTokenLine('"cache_creation_input_tokens": 1')).toBe(1);
    expect(firstTokenLine("let anthropic_cached_tokens = 1;")).toBe(1);
    expect(firstTokenLine("input_tokens")).toBe(0);
  });

  it("allows codec/, usage/ and the allowlist, flags everything else", () => {
    const { violations, stale } = checkUsageTokenNames(
      {
        "src-tauri/src/proxy/codec/responses/usage.rs": "cached_tokens",
        "src-tauri/src/proxy/usage/parser.rs": "cache_creation_input_tokens",
        "src-tauri/src/proxy/legacy.rs": "x\ncached_tokens",
        "src-tauri/src/proxy/handlers.rs": "a\nb\ncache_creation_input_tokens",
        "src-tauri/src/proxy/clean.rs": "input_tokens",
        "src-tauri/src/proxy/gone.rs": "nothing here",
      },
      {
        "src-tauri/src/proxy/legacy.rs": "remove later",
        "src-tauri/src/proxy/gone.rs": "remove later",
      },
    );
    expect(violations).toEqual([
      expect.objectContaining({
        file: "src-tauri/src/proxy/handlers.rs",
        line: 3,
      }),
    ]);
    expect(stale).toEqual(["src-tauri/src/proxy/gone.rs"]);
  });

  it("keeps a removal note on every allowlist entry", () => {
    for (const reason of Object.values(ALLOWLIST)) {
      expect(reason).toMatch(/^remove (with|when) CCS-\d+/);
    }
  });

  it("passes on the real source tree", () => {
    const r = run();
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toMatch(/cache-token field names stay in/);
  });

  it("fails when a new file outside codec/ and usage/ names the field", () => {
    const root = mkdtempSync(path.join(tmpdir(), "ccs243-"));
    tmpDirs.push(root);
    const write = (rel: string, body: string) => {
      const full = path.join(root, rel);
      mkdirSync(path.dirname(full), { recursive: true });
      writeFileSync(full, body);
    };
    for (const file of Object.keys(ALLOWLIST)) write(file, "cached_tokens\n");
    write("src-tauri/src/proxy/usage/parser.rs", "cached_tokens\n");
    expect(run(["--root", root]).status).toBe(0);

    write("src-tauri/src/proxy/handlers.rs", "let _ = u.cached_tokens;\n");
    const r = run(["--root", root]);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("src-tauri/src/proxy/handlers.rs:1");
  });
});
