#!/usr/bin/env node
// Check that the app version is consistent before a release (CCS-033).
//
// Usage:
//   node scripts/check-versions.mjs [tag] [--root <dir>]
//
// package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json must carry
// the same version, and CHANGELOG.md must have a `## [<version>]` entry (a
// prerelease version such as 3.18.0-rc.1 may use `## [Unreleased]` instead).
// When a tag is given (e.g. v3.18.0) it must equal `v<version>`. Without an
// argument the tag is taken from GITHUB_REF when it is refs/tags/*, so CI on a
// tag push (release.yml calls ci.yml) checks the tag too.
//
// Exits 1 and lists every mismatch on stderr when a check fails.
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Read the `version` key of the [package] table from Cargo.toml text. */
export function cargoPackageVersion(text) {
  let inPackage = false;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    const table = line.match(/^\[([^\]]+)\]/);
    if (table) {
      inPackage = table[1].trim() === "package";
      continue;
    }
    if (!inPackage) continue;
    const m = line.match(/^version\s*=\s*"([^"]*)"/);
    if (m) return m[1];
  }
  return null;
}

/** Read the three manifest versions and CHANGELOG.md below `root`. */
export function readVersions(root) {
  const read = (rel) => readFileSync(path.join(root, rel), "utf8");
  const packageJson = JSON.parse(read("package.json")).version ?? null;
  const cargoToml = cargoPackageVersion(read("src-tauri/Cargo.toml"));
  let tauriConf = JSON.parse(read("src-tauri/tauri.conf.json")).version ?? null;
  // Tauri 2 also accepts a path to a JSON file that holds the version.
  if (typeof tauriConf === "string" && tauriConf.endsWith(".json")) {
    const file = path.resolve(root, "src-tauri", tauriConf);
    tauriConf = JSON.parse(readFileSync(file, "utf8")).version ?? null;
  }
  return {
    versions: {
      "package.json": packageJson,
      "src-tauri/Cargo.toml": cargoToml,
      "src-tauri/tauri.conf.json": tauriConf,
    },
    changelog: read("CHANGELOG.md"),
  };
}

const escapeRegExp = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Whether CHANGELOG text has a `## [version]` heading. */
export function hasChangelogEntry(changelog, version) {
  const heading = (v) => new RegExp(`^##\\s+\\[${escapeRegExp(v)}\\]`, "m");
  if (heading(version).test(changelog)) return true;
  return version.includes("-") && heading("Unreleased").test(changelog);
}

/** Return a list of problems; an empty list means the versions are consistent. */
export function checkVersions({ tag, versions, changelog }) {
  const errors = [];
  for (const [file, v] of Object.entries(versions)) {
    if (!v) errors.push(`${file}: no version found`);
  }
  const distinct = [...new Set(Object.values(versions).filter(Boolean))];
  if (distinct.length > 1) {
    const list = Object.entries(versions)
      .map(([file, v]) => `${file}=${v}`)
      .join(", ");
    errors.push(`versions differ: ${list}`);
  }
  const version = distinct.length === 1 ? distinct[0] : null;
  if (tag) {
    for (const [file, v] of Object.entries(versions)) {
      if (v && tag !== `v${v}`) {
        errors.push(`tag ${tag} does not match ${file} version ${v}`);
      }
    }
  }
  if (version && !hasChangelogEntry(changelog, version)) {
    errors.push(`CHANGELOG.md has no "## [${version}]" entry`);
  }
  return errors;
}

function parseArgs(argv) {
  const opts = { tag: null, root: process.cwd() };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--root") {
      if (i + 1 >= argv.length) throw new Error("--root needs a value");
      opts.root = path.resolve(argv[++i]);
    } else if (arg.startsWith("-")) throw new Error(`unknown argument: ${arg}`);
    else if (opts.tag === null) opts.tag = arg;
    else throw new Error(`unexpected argument: ${arg}`);
  }
  if (opts.tag === null) {
    const ref = process.env.GITHUB_REF ?? "";
    if (ref.startsWith("refs/tags/")) opts.tag = ref.slice("refs/tags/".length);
  }
  return opts;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const { versions, changelog } = readVersions(opts.root);
  const errors = checkVersions({ tag: opts.tag, versions, changelog });
  if (errors.length > 0) {
    for (const e of errors) console.error(`❌ ${e}`);
    process.exit(1);
  }
  const version = versions["package.json"];
  const against = opts.tag ? ` (tag ${opts.tag})` : "";
  console.log(`✅ version ${version} is consistent${against}`);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main();
  } catch (err) {
    console.error(`❌ ${err instanceof Error ? err.message : err}`);
    process.exit(1);
  }
}
