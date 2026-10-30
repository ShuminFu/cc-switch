#!/usr/bin/env node
// Single entry point for the app version (CI-06).
//
// The version is embedded in four files that must always agree:
//   package.json               "version"
//   src-tauri/Cargo.toml       [package] version
//   src-tauri/tauri.conf.json  "version"   (what the updater compares against)
//   src-tauri/Cargo.lock       the cc-switch [[package]] entry
//
// Usage:
//   node scripts/bump-version.mjs <version>        rewrite all four files
//   node scripts/bump-version.mjs --check          fail unless all four agree
//   node scripts/bump-version.mjs --check --tag v3.18.0
//                                                  ...and agree with the tag
//   --root <dir>                                   operate on another checkout
//
// Edits are textual so formatting, key order and comments are preserved.
// Moving the CHANGELOG [Unreleased] section, committing and tagging stay
// manual (see CONTRIBUTING.md); run `--check` before pushing the tag.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

// SemVer 2.0.0 (prerelease and build metadata allowed).
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

const PACKAGE_NAME = "cc-switch";

/**
 * Each target knows how to read and rewrite its version. `pattern` must have
 * three groups: prefix, version, suffix.
 */
const TARGETS = [
  {
    file: "package.json",
    // First top-level "version" key (two-space indented, as written by pnpm).
    pattern: /^(\s{2}"version"\s*:\s*")([^"]*)(")/m,
  },
  {
    file: "src-tauri/tauri.conf.json",
    pattern: /^(\s{2}"version"\s*:\s*")([^"]*)(")/m,
  },
  {
    file: "src-tauri/Cargo.toml",
    // `version = ` inside the [package] table (before the next table header).
    pattern:
      /(^\[package\]\r?\n(?:(?!\[)[^\n]*\n)*?version\s*=\s*")([^"]*)(")/m,
  },
  {
    file: "src-tauri/Cargo.lock",
    pattern: new RegExp(
      `(^\\[\\[package\\]\\]\\r?\\nname = "${PACKAGE_NAME}"\\r?\\nversion = ")([^"]*)(")`,
      "m",
    ),
  },
];

function parseArgs(argv) {
  const opts = { check: false, tag: null, version: null, root: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--check") opts.check = true;
    else if (arg === "--tag") opts.tag = argv[++i] ?? "";
    else if (arg === "--root") opts.root = argv[++i] ?? "";
    else if (arg === "-h" || arg === "--help") opts.help = true;
    else if (arg.startsWith("-")) throw new Error(`Unknown option: ${arg}`);
    else if (opts.version === null) opts.version = arg;
    else throw new Error(`Unexpected argument: ${arg}`);
  }
  return opts;
}

function usage() {
  return [
    "Usage:",
    "  node scripts/bump-version.mjs <version>",
    "  node scripts/bump-version.mjs --check [--tag vX.Y.Z]",
  ].join("\n");
}

function readVersions(root) {
  return TARGETS.map((t) => {
    const abs = path.join(root, t.file);
    const text = fs.readFileSync(abs, "utf8");
    const m = t.pattern.exec(text);
    if (!m) {
      throw new Error(`${t.file}: could not find the version field`);
    }
    return { ...t, abs, text, version: m[2] };
  });
}

function check(root, tag) {
  const entries = readVersions(root);
  const errors = [];
  const expected = entries[0].version;
  for (const e of entries) {
    if (e.version !== expected) {
      errors.push(
        `${e.file} has ${e.version}, expected ${expected} (from package.json)`,
      );
    }
  }
  if (tag !== null) {
    const tagVersion = tag.replace(/^refs\/tags\//, "").replace(/^v/, "");
    if (tagVersion !== expected) {
      errors.push(
        `tag ${tag} does not match the embedded version ${expected}; ` +
          "the updater would offer a release that reinstalls the same build",
      );
    }
  }
  for (const e of entries) console.log(`  ${e.file}: ${e.version}`);
  if (errors.length > 0) {
    for (const err of errors) console.error(`error: ${err}`);
    console.error(
      "Run `node scripts/bump-version.mjs <version>` to bring them in line.",
    );
    return 1;
  }
  console.log(
    `Version ${expected} is consistent across ${entries.length} files` +
      (tag !== null ? ` and matches tag ${tag}.` : "."),
  );
  return 0;
}

function bump(root, version) {
  const normalized = version.replace(/^v/, "");
  if (!SEMVER.test(normalized)) {
    throw new Error(`Not a valid semver version: ${version}`);
  }
  const entries = readVersions(root);
  // Compute every edit before writing anything so a failure leaves no
  // half-bumped tree behind.
  const updates = entries.map((e) => ({
    ...e,
    next: e.text.replace(e.pattern, `$1${normalized}$3`),
  }));
  for (const u of updates) {
    if (u.next !== u.text) fs.writeFileSync(u.abs, u.next);
    console.log(`  ${u.file}: ${u.version} -> ${normalized}`);
  }
  console.log(
    `Bumped to ${normalized}. Next: move the CHANGELOG [Unreleased] entries, ` +
      `commit "chore(release): v${normalized}", then tag v${normalized}.`,
  );
  return 0;
}

function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(`error: ${err.message}\n${usage()}`);
    return 2;
  }
  if (opts.help) {
    console.log(usage());
    return 0;
  }
  const root = path.resolve(
    opts.root || path.join(path.dirname(fileURLToPath(import.meta.url)), ".."),
  );
  try {
    if (opts.check) {
      if (opts.version !== null) {
        throw new Error("--check does not take a version argument");
      }
      return check(root, opts.tag);
    }
    if (opts.version === null) {
      console.error(usage());
      return 2;
    }
    return bump(root, opts.version);
  } catch (err) {
    console.error(`error: ${err.message}`);
    return 1;
  }
}

process.exitCode = main();
