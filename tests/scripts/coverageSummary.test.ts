import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it, expect } from "vitest";
import {
  checkThresholds,
  formatFailure,
  parseLcov,
  parseMin,
  summarize,
  toMarkdown,
} from "../../scripts/coverage-summary.mjs";

const SCRIPT = path.resolve(__dirname, "../../scripts/coverage-summary.mjs");

// CI 中 coverage 任务依赖该脚本把 LCOV 汇总为 proxy/、database/ 等目录的覆盖率表格。
const ROOT = path.resolve("/repo");

const lcov = [
  "TN:",
  `SF:${ROOT}/src-tauri/src/proxy/handler.rs`,
  "FNF:4",
  "FNH:2",
  "LF:100",
  "LH:80",
  "end_of_record",
  `SF:${ROOT}/src-tauri/src/proxy/mod.rs`,
  "FNF:0",
  "FNH:0",
  "LF:0",
  "LH:0",
  "end_of_record",
  "SF:src-tauri/src/database/dao.rs",
  "FNF:2",
  "FNH:1",
  "LF:50",
  "LH:10",
  "end_of_record",
  // 前缀相同但不是子目录，不应计入 proxy/
  "SF:src-tauri/src/proxy_utils.rs",
  "LF:10",
  "LH:10",
  "end_of_record",
  "",
].join("\n");

describe("coverage-summary", () => {
  it("parses LCOV records", () => {
    const records = parseLcov(lcov);
    expect(records).toHaveLength(4);
    expect(records[0]).toEqual({
      file: `${ROOT}/src-tauri/src/proxy/handler.rs`,
      linesFound: 100,
      linesHit: 80,
      functionsFound: 4,
      functionsHit: 2,
    });
    expect(records[3].functionsFound).toBe(0);
  });

  it("groups absolute and relative paths per directory", () => {
    const rows = summarize(
      parseLcov(lcov),
      ["src-tauri/src/proxy", "src-tauri/src/database/"],
      ROOT,
    );
    const byDir = Object.fromEntries(rows.map((r) => [r.dir, r]));

    expect(byDir["src-tauri/src/proxy"]).toMatchObject({
      files: 2,
      linesFound: 100,
      linesHit: 80,
      linesPct: 80,
      functionsPct: 50,
    });
    expect(byDir["src-tauri/src/database"]).toMatchObject({
      files: 1,
      linesPct: 20,
      functionsPct: 50,
    });
    expect(byDir.total).toMatchObject({
      files: 4,
      linesFound: 160,
      linesHit: 100,
      linesPct: 62.5,
    });
  });

  it("reports n/a for directories without instrumented code", () => {
    const rows = summarize(parseLcov(lcov), ["src-tauri/src/missing"], ROOT);
    expect(rows[0]).toMatchObject({ files: 0, linesPct: null });

    const md = toMarkdown("Rust", rows);
    expect(md).toContain("### Coverage: Rust");
    expect(md).toContain("| `src-tauri/src/missing/` | 0 | n/a (0/0) |");
    expect(md).toContain("| **total** | 4 | 62.50% (100/160) |");
  });
});

describe("coverage-summary --min thresholds", () => {
  const rows = summarize(
    parseLcov(lcov),
    ["src-tauri/src/proxy", "src-tauri/src/database", "src-tauri/src/missing"],
    ROOT,
  );

  it("parses <dir>=<pct> floors", () => {
    expect(parseMin("src-tauri/src/proxy=75")).toEqual({
      dir: "src-tauri/src/proxy",
      min: 75,
    });
    expect(parseMin("src-tauri\\src\\database/=62.5")).toEqual({
      dir: "src-tauri/src/database",
      min: 62.5,
    });
    for (const bad of ["proxy", "=75", "proxy=", "proxy=abc", "proxy=101"]) {
      expect(() => parseMin(bad)).toThrow(/invalid --min/);
    }
  });

  it("passes when every directory meets its floor", () => {
    expect(
      checkThresholds(rows, [
        { dir: "src-tauri/src/proxy", min: 80 },
        { dir: "src-tauri/src/database", min: 20 },
      ]),
    ).toEqual([]);
  });

  it("names directories below the floor or without instrumented lines", () => {
    const failures = checkThresholds(rows, [
      { dir: "src-tauri/src/proxy", min: 75 },
      { dir: "src-tauri/src/database", min: 75 },
      { dir: "src-tauri/src/missing", min: 75 },
    ]);
    expect(failures).toEqual([
      { dir: "src-tauri/src/database", min: 75, actual: 20 },
      { dir: "src-tauri/src/missing", min: 75, actual: null },
    ]);
    expect(formatFailure(failures[0])).toBe(
      "coverage below floor: src-tauri/src/database/ lines 20.00% < min 75%",
    );
    expect(formatFailure(failures[1])).toContain("no instrumented lines");
  });

  describe("CLI exit code", () => {
    const run = (...args: string[]) => {
      const dir = mkdtempSync(path.join(os.tmpdir(), "cov-min-"));
      try {
        // The script relativises SF: paths against its cwd.
        const file = path.join(dir, "lcov.info");
        writeFileSync(file, lcov.split(ROOT).join(dir));
        return spawnSync(
          process.execPath,
          [SCRIPT, "--title", "Rust", "--lcov", file, ...args],
          { cwd: dir, encoding: "utf8" },
        );
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    };

    it("exits 0 when coverage is above the floor", () => {
      const res = run(
        "--dir",
        "src-tauri/src/proxy",
        "--min",
        "src-tauri/src/proxy=75",
      );
      expect(res.status).toBe(0);
      expect(res.stderr).toBe("");
      expect(res.stdout).toContain("`src-tauri/src/proxy/` >= 75%): passed");
    });

    it("exits 1 and names the failing directory when below the floor", () => {
      // database/ is not passed via --dir; --min adds it to the table.
      const res = run(
        "--dir",
        "src-tauri/src/proxy",
        "--min",
        "src-tauri/src/proxy=75",
        "--min",
        "src-tauri/src/database=75",
      );
      expect(res.status).toBe(1);
      expect(res.stdout).toContain("| `src-tauri/src/database/` | 1 |");
      expect(res.stdout).toContain("FAILED");
      expect(res.stderr).toContain(
        "coverage below floor: src-tauri/src/database/ lines 20.00% < min 75%",
      );
      expect(res.stderr).not.toContain("src-tauri/src/proxy/");
    });
  });
});
