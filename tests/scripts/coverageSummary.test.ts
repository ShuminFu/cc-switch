import path from "node:path";
import { describe, it, expect } from "vitest";
import {
  parseLcov,
  summarize,
  toMarkdown,
} from "../../scripts/coverage-summary.mjs";

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
