#!/usr/bin/env node
// Require a golden fixture for every proxy change in a PR (CCS-034).
//
// Usage:
//   node scripts/check-proxy-fixture.mjs [--base <sha>] [--head <sha>] [--label <name>]...
//
// A PR that changes anything under src-tauri/src/proxy/ must also change a
// file under src-tauri/tests/fixtures/proxy/ (the golden bridge corpus), or
// carry the `no-fixture-needed` label. The changed files are
// `git diff --name-only <base>...<head>`; base and head default to the
// BASE_SHA and HEAD_SHA environment variables, and PR_LABELS may hold the
// PR's label names as a JSON array (proxy-fixture.yml passes all three).
//
// Exits 1 and lists the proxy files on stderr when the rule is broken.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PROXY_PREFIX = "src-tauri/src/proxy/";
export const FIXTURE_PREFIX = "src-tauri/tests/fixtures/proxy/";
export const SKIP_LABEL = "no-fixture-needed";

/** Parse the PR_LABELS value: a JSON array of names, or empty. */
export function parseLabels(raw) {
  if (!raw || !raw.trim()) return [];
  const value = JSON.parse(raw);
  if (!Array.isArray(value)) throw new Error("PR_LABELS must be a JSON array");
  return value.map(String);
}

/** Apply the rule to the changed files and the PR labels. */
export function checkProxyFixture({ files, labels }) {
  const norm = files.map((f) => f.replace(/\\/g, "/"));
  const proxyFiles = norm.filter((f) => f.startsWith(PROXY_PREFIX));
  const fixtureFiles = norm.filter((f) => f.startsWith(FIXTURE_PREFIX));
  const skipped = labels.includes(SKIP_LABEL);
  const ok = proxyFiles.length === 0 || fixtureFiles.length > 0 || skipped;
  return { ok, proxyFiles, fixtureFiles, skipped };
}

/** Files changed between the merge base of base and head, and head. */
export function changedFiles(base, head, cwd = process.cwd()) {
  const out = execFileSync(
    "git",
    ["diff", "--name-only", "--no-renames", `${base}...${head}`],
    { cwd, encoding: "utf8" },
  );
  return out.split(/\r?\n/).filter(Boolean);
}

function main(argv) {
  let base = process.env.BASE_SHA || "";
  let head = process.env.HEAD_SHA || "HEAD";
  const labels = parseLabels(process.env.PR_LABELS);
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      return argv[++i];
    };
    if (arg === "--base") base = next();
    else if (arg === "--head") head = next();
    else if (arg === "--label") labels.push(next());
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!base) throw new Error("no base: pass --base or set BASE_SHA");

  const result = checkProxyFixture({
    files: changedFiles(base, head),
    labels,
  });
  if (result.proxyFiles.length === 0) {
    console.log("✅ no change under src-tauri/src/proxy/");
  } else if (result.fixtureFiles.length > 0) {
    console.log(
      `✅ proxy change comes with ${result.fixtureFiles.length} fixture file(s) under ${FIXTURE_PREFIX}`,
    );
  } else if (result.skipped) {
    console.log(`✅ proxy change without a fixture, labelled ${SKIP_LABEL}`);
  } else {
    console.error(
      `❌ this PR changes ${result.proxyFiles.length} file(s) under ${PROXY_PREFIX}:`,
    );
    for (const f of result.proxyFiles) console.error(`   - ${f}`);
    console.error(
      `   Add or update a golden fixture under ${FIXTURE_PREFIX}, or ask a`,
    );
    console.error(
      `   maintainer for the \`${SKIP_LABEL}\` label (see docs/dev/ACCEPTANCE.md).`,
    );
  }
  return result.ok ? 0 : 1;
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
