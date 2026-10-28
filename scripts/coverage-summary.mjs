#!/usr/bin/env node
// Summarise an LCOV report per directory (report-only; no thresholds).
//
// Usage:
//   node scripts/coverage-summary.mjs --title "Rust" --lcov lcov.info \
//     --dir src-tauri/src/proxy --dir src-tauri/src/database \
//     [--json out.json]
//
// Prints a Markdown table to stdout (CI appends it to $GITHUB_STEP_SUMMARY).
// Directories are matched against SF: paths relative to the repository root;
// absolute paths (as written by cargo-llvm-cov) are made relative first.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const emptyCounts = () => ({
  files: 0,
  linesFound: 0,
  linesHit: 0,
  functionsFound: 0,
  functionsHit: 0,
});

/** Parse LCOV text into per-file line/function counters. */
export function parseLcov(text) {
  const records = [];
  let current = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("SF:")) {
      current = {
        file: line.slice(3),
        linesFound: 0,
        linesHit: 0,
        functionsFound: 0,
        functionsHit: 0,
      };
    } else if (!current) {
      continue;
    } else if (line.startsWith("LF:")) {
      current.linesFound = Number(line.slice(3));
    } else if (line.startsWith("LH:")) {
      current.linesHit = Number(line.slice(3));
    } else if (line.startsWith("FNF:")) {
      current.functionsFound = Number(line.slice(4));
    } else if (line.startsWith("FNH:")) {
      current.functionsHit = Number(line.slice(4));
    } else if (line === "end_of_record") {
      records.push(current);
      current = null;
    }
  }
  return records;
}

const toPosix = (p) => p.replace(/\\/g, "/");

const pct = (hit, found) =>
  found === 0 ? null : Math.round((hit / found) * 10000) / 100;

/**
 * Group LCOV records by directory prefix. `root` is the repository root used
 * to relativise absolute SF: paths. Returns one row per directory plus "total".
 */
export function summarize(records, dirs, root = process.cwd()) {
  const rootPosix = toPosix(path.resolve(root)).replace(/\/$/, "");
  const groups = dirs.map((dir) => ({
    dir: toPosix(dir).replace(/\/$/, ""),
    ...emptyCounts(),
  }));
  const total = { dir: "total", ...emptyCounts() };

  const add = (acc, r) => {
    acc.files += 1;
    acc.linesFound += r.linesFound;
    acc.linesHit += r.linesHit;
    acc.functionsFound += r.functionsFound;
    acc.functionsHit += r.functionsHit;
  };

  for (const r of records) {
    let file = toPosix(r.file);
    if (file.startsWith(`${rootPosix}/`)) {
      file = file.slice(rootPosix.length + 1);
    }
    add(total, r);
    for (const g of groups) {
      if (file.startsWith(`${g.dir}/`)) add(g, r);
    }
  }

  return [...groups, total].map((g) => ({
    ...g,
    linesPct: pct(g.linesHit, g.linesFound),
    functionsPct: pct(g.functionsHit, g.functionsFound),
  }));
}

const fmtPct = (v) => (v === null ? "n/a" : `${v.toFixed(2)}%`);

export function toMarkdown(title, rows) {
  const out = [
    `### Coverage: ${title}`,
    "",
    "| Directory | Files | Lines | Functions |",
    "| --- | ---: | ---: | ---: |",
  ];
  for (const r of rows) {
    const name = r.dir === "total" ? "**total**" : `\`${r.dir}/\``;
    out.push(
      `| ${name} | ${r.files} | ${fmtPct(r.linesPct)} (${r.linesHit}/${r.linesFound}) | ${fmtPct(r.functionsPct)} (${r.functionsHit}/${r.functionsFound}) |`,
    );
  }
  return `${out.join("\n")}\n`;
}

function parseArgs(argv) {
  const opts = { title: "coverage", lcov: null, dirs: [], json: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`missing value for ${arg}`);
      return v;
    };
    if (arg === "--title") opts.title = next();
    else if (arg === "--lcov") opts.lcov = next();
    else if (arg === "--dir") opts.dirs.push(next());
    else if (arg === "--json") opts.json = next();
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!opts.lcov) throw new Error("--lcov is required");
  return opts;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const rows = summarize(parseLcov(readFileSync(opts.lcov, "utf8")), opts.dirs);
  process.stdout.write(toMarkdown(opts.title, rows));
  if (opts.json) {
    writeFileSync(
      opts.json,
      `${JSON.stringify({ title: opts.title, rows }, null, 2)}\n`,
    );
  }
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
