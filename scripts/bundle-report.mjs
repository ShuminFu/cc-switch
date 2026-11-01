#!/usr/bin/env node
// Renderer bundle size report and budget check (PERF-29, PERF-30).
//
//   pnpm build:renderer && node scripts/bundle-report.mjs
//   node scripts/bundle-report.mjs --summary "$GITHUB_STEP_SUMMARY" --json reports/bundle-size.json
//
// Reads dist/ (the Vite output), lists every JS/CSS asset with its raw and
// gzip size, and fails when a budget below is exceeded. The "entry" budget
// covers what the webview must parse at startup: the JS/CSS referenced
// directly by dist/index.html, including <link rel="modulepreload"> chunks
// (lazy chunks are loaded on demand).
//
// For a treemap of what is inside a chunk, run `pnpm bundle:visualize`.

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Budgets in gzip KiB, ~25% above the v3.17 baseline (entry 719 KiB incl.
// modulepreloaded vendor chunks, all JS 1269 KiB, largest chunk 473 KiB).
// Raise them deliberately (with a reason in the PR) when a feature needs it;
// lower them after a size win so it cannot regress.
export const BUDGETS = {
  entryJsGzipKiB: 900,
  totalJsGzipKiB: 1600,
  largestChunkGzipKiB: 600,
};

function walk(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

export function collect(distDir) {
  const html = fs.existsSync(path.join(distDir, "index.html"))
    ? fs.readFileSync(path.join(distDir, "index.html"), "utf8")
    : "";
  const entryRefs = new Set(
    [...html.matchAll(/(?:src|href)="\.?\/?([^"]+\.(?:js|css))"/g)].map(
      (m) => m[1],
    ),
  );
  return walk(distDir)
    .filter((f) => /\.(js|css)$/.test(f))
    .map((f) => {
      const buf = fs.readFileSync(f);
      const rel = path.relative(distDir, f).split(path.sep).join("/");
      return {
        file: rel,
        type: f.endsWith(".js") ? "js" : "css",
        bytes: buf.length,
        gzipBytes: zlib.gzipSync(buf, { level: 9 }).length,
        entry: entryRefs.has(rel),
      };
    })
    .sort((a, b) => b.gzipBytes - a.gzipBytes);
}

const kib = (n) => n / 1024;
const fmt = (n) => `${kib(n).toFixed(1)} KiB`;

export function evaluate(assets, budgets = BUDGETS) {
  const js = assets.filter((a) => a.type === "js");
  const entryJs = js.filter((a) => a.entry);
  const totals = {
    entryJsGzip: entryJs.reduce((s, a) => s + a.gzipBytes, 0),
    totalJsGzip: js.reduce((s, a) => s + a.gzipBytes, 0),
    largestChunkGzip: js[0]?.gzipBytes ?? 0,
  };
  const failures = [];
  if (kib(totals.entryJsGzip) > budgets.entryJsGzipKiB)
    failures.push(
      `entry JS ${fmt(totals.entryJsGzip)} > ${budgets.entryJsGzipKiB} KiB`,
    );
  if (kib(totals.totalJsGzip) > budgets.totalJsGzipKiB)
    failures.push(
      `total JS ${fmt(totals.totalJsGzip)} > ${budgets.totalJsGzipKiB} KiB`,
    );
  if (kib(totals.largestChunkGzip) > budgets.largestChunkGzipKiB)
    failures.push(
      `largest chunk ${js[0]?.file} ${fmt(totals.largestChunkGzip)} > ${budgets.largestChunkGzipKiB} KiB`,
    );
  return { totals, failures };
}

function markdown(assets, { totals, failures }) {
  const rows = assets
    .slice(0, 25)
    .map(
      (a) =>
        `| \`${a.file}\` | ${a.entry ? "entry" : "lazy"} | ${fmt(a.bytes)} | ${fmt(a.gzipBytes)} |`,
    );
  return [
    "## Renderer bundle size",
    "",
    `Entry JS (parsed at startup): **${fmt(totals.entryJsGzip)}** gzip (budget ${BUDGETS.entryJsGzipKiB} KiB) · ` +
      `all JS: **${fmt(totals.totalJsGzip)}** gzip (budget ${BUDGETS.totalJsGzipKiB} KiB)`,
    "",
    failures.length
      ? `❌ Over budget: ${failures.join("; ")}`
      : "✅ Within budget",
    "",
    "| Asset | Loaded | Raw | Gzip |",
    "| --- | --- | --- | --- |",
    ...rows,
    assets.length > 25 ? `\n… and ${assets.length - 25} smaller assets` : "",
    "",
  ].join("\n");
}

function main() {
  const args = process.argv.slice(2);
  const opt = (name) => {
    const i = args.indexOf(name);
    return i === -1 ? undefined : args[i + 1];
  };
  const dist = path.resolve(ROOT, opt("--dist") ?? "dist");
  if (!fs.existsSync(dist)) {
    console.error(
      `${path.relative(ROOT, dist)}/ not found; run \`pnpm build:renderer\` first`,
    );
    process.exit(2);
  }
  const assets = collect(dist);
  const result = evaluate(assets);
  const md = markdown(assets, result);
  console.log(md);
  const summary = opt("--summary");
  if (summary) fs.appendFileSync(summary, md);
  const json = opt("--json");
  if (json) {
    fs.mkdirSync(path.dirname(path.resolve(json)), { recursive: true });
    fs.writeFileSync(
      json,
      JSON.stringify({ budgets: BUDGETS, ...result, assets }, null, 2),
    );
  }
  if (result.failures.length) process.exit(1);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
