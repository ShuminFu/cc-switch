#!/usr/bin/env node
// Keep cache-token field names inside the codec and usage modules (CCS-243).
//
// Usage:
//   node scripts/check-usage-token-names.mjs [--root <dir>]
//
// Scans every .rs file under src-tauri/src for `cached_tokens` or
// `cache_creation_input_tokens` (substring match, like `rg`) and fails on any match outside
// `src-tauri/src/proxy/codec/` and `src-tauri/src/proxy/usage/`, unless the
// file is on the allowlist below. An allowlisted file that no longer mentions
// the names also fails, so the allowlist shrinks as the legacy code goes.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const TOKEN_NAMES = /cached_tokens|cache_creation_input_tokens/;

export const ALLOWED_DIRS = [
  "src-tauri/src/proxy/codec/",
  "src-tauri/src/proxy/usage/",
];

// 允许暂时保留这些字段名的文件及其移除条件
export const ALLOWLIST = {
  "src-tauri/src/proxy/providers/streaming.rs": "remove with CCS-178 deletion",
  "src-tauri/src/proxy/providers/transform.rs": "remove with CCS-178 deletion",
  "src-tauri/src/proxy/providers/transform_codex_anthropic.rs":
    "remove with CCS-178 deletion",
  "src-tauri/src/proxy/providers/transform_codex_chat.rs":
    "remove with CCS-178 deletion",
  "src-tauri/src/proxy/providers/transform_responses.rs":
    "remove with CCS-178 deletion",
};

const toPosix = (p) => p.split(path.sep).join("/");

/** First line (1-based) of `source` that names a cache-token field, or 0. */
export function firstTokenLine(source) {
  const lines = source.split("\n");
  const idx = lines.findIndex((l) => TOKEN_NAMES.test(l));
  return idx + 1;
}

/** Check every file: `files` maps a repo-relative path to its Rust source. */
export function checkUsageTokenNames(files, allowlist = ALLOWLIST) {
  const violations = [];
  const seen = new Set();
  for (const [rawFile, source] of Object.entries(files)) {
    const file = toPosix(rawFile);
    const line = firstTokenLine(source);
    if (line === 0) continue;
    if (ALLOWED_DIRS.some((dir) => file.startsWith(dir))) continue;
    if (Object.hasOwn(allowlist, file)) {
      seen.add(file);
      continue;
    }
    violations.push({
      file,
      line,
      reason:
        "cache-token field name outside proxy/codec/ and proxy/usage/ (move the usage mapping there)",
    });
  }
  const stale = Object.keys(allowlist)
    .filter((file) => !seen.has(file))
    .sort();
  violations.sort((a, b) => a.file.localeCompare(b.file));
  return { violations, stale };
}

function walkRust(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkRust(full, out);
    else if (entry.name.endsWith(".rs")) out.push(full);
  }
  return out;
}

/** Read every .rs file under `<root>/src-tauri/src`. */
export function readRustSources(root) {
  const base = path.join(root, "src-tauri/src");
  const files = {};
  for (const full of walkRust(base)) {
    files[toPosix(path.relative(root, full))] = readFileSync(full, "utf8");
  }
  return files;
}

function main(argv) {
  let root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--root") root = path.resolve(argv[++i]);
    else throw new Error(`unknown argument: ${argv[i]}`);
  }

  const { violations, stale } = checkUsageTokenNames(readRustSources(root));
  for (const v of violations) {
    console.error(`${v.file}:${v.line} ${v.reason}`);
  }
  for (const file of stale) {
    console.error(
      `${file}: allowlisted but no longer names a cache-token field (drop it from ALLOWLIST)`,
    );
  }
  const failures = violations.length + stale.length;
  if (failures > 0) {
    console.error(`❌ ${failures} cache-token name violation(s)`);
    return 1;
  }
  console.log(
    `✅ cache-token field names stay in proxy/codec/ and proxy/usage/ (${Object.keys(ALLOWLIST).length} allowlisted legacy files)`,
  );
  return 0;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (err) {
    console.error(`❌ ${err instanceof Error ? err.message : err}`);
    process.exit(1);
  }
}
