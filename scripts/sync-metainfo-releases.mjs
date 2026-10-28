#!/usr/bin/env node
// Generate the AppStream <releases> block of the Flatpak metainfo (CCS-044).
//
// Usage:
//   node scripts/sync-metainfo-releases.mjs [--check] [--root <dir>]
//
// The block lists the newest dated `## [x.y.z] - YYYY-MM-DD` entries of
// CHANGELOG.md (at most MAX_RELEASES). The package.json version must be among
// them unless it is a prerelease (e.g. 3.18.0-rc.1), which is not published to
// AppStream. Without --check the metainfo is rewritten in place; with --check
// nothing is written and the script exits 1 when the file is out of date, so
// CI catches a version bump that forgot to regenerate it.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const METAINFO = "flatpak/com.ccswitch.desktop.metainfo.xml";
export const MAX_RELEASES = 10;
const RELEASE_URL = "https://github.com/farion1231/cc-switch/releases/tag/v";

/** Dated release entries of CHANGELOG text, newest first as written. */
export function changelogReleases(changelog) {
  const re = /^##\s+\[(\d+\.\d+\.\d+)\]\s*-\s*(\d{4}-\d{2}-\d{2})\s*$/gm;
  return [...changelog.matchAll(re)].map((m) => ({
    version: m[1],
    date: m[2],
  }));
}

/** Render the <releases> element, indented for a direct child of <component>. */
export function renderReleases(releases) {
  const lines = ["  <releases>"];
  for (const { version, date } of releases) {
    lines.push(`    <release version="${version}" date="${date}">`);
    lines.push(`      <url type="details">${RELEASE_URL}${version}</url>`);
    lines.push("    </release>");
  }
  lines.push("  </releases>");
  return lines.join("\n");
}

/** Replace (or insert before </component>) the <releases> block of metainfo XML. */
export function applyReleases(xml, block) {
  const existing = /^[ \t]*<releases>[\s\S]*?<\/releases>[ \t]*$/m;
  if (existing.test(xml)) return xml.replace(existing, block);
  if (!xml.includes("</component>")) {
    throw new Error("metainfo has no </component> element");
  }
  return xml.replace("</component>", `${block}\n</component>`);
}

/** Compute the expected metainfo text; throws when the version has no entry. */
export function syncMetainfo({ xml, version, changelog }) {
  const releases = changelogReleases(changelog).slice(0, MAX_RELEASES);
  if (!version.includes("-") && !releases.some((r) => r.version === version)) {
    throw new Error(
      `CHANGELOG.md has no dated "## [${version}] - YYYY-MM-DD" entry`,
    );
  }
  if (releases.length === 0) throw new Error("CHANGELOG.md has no releases");
  return applyReleases(xml, renderReleases(releases));
}

function parseArgs(argv) {
  const opts = { check: false, root: process.cwd() };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--check") opts.check = true;
    else if (arg === "--root") {
      if (i + 1 >= argv.length) throw new Error("--root needs a value");
      opts.root = path.resolve(argv[++i]);
    } else throw new Error(`unknown argument: ${arg}`);
  }
  return opts;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const read = (rel) => readFileSync(path.join(opts.root, rel), "utf8");
  const version = JSON.parse(read("package.json")).version;
  const xml = read(METAINFO);
  const next = syncMetainfo({ xml, version, changelog: read("CHANGELOG.md") });
  if (next === xml) {
    console.log(`✅ ${METAINFO} releases are up to date (${version})`);
    return;
  }
  if (opts.check) {
    console.error(
      `❌ ${METAINFO} releases are out of date; run node scripts/sync-metainfo-releases.mjs`,
    );
    process.exit(1);
  }
  writeFileSync(path.join(opts.root, METAINFO), next);
  console.log(`✅ updated ${METAINFO} releases (${version})`);
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
