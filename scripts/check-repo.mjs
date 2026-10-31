#!/usr/bin/env node
// Repository consistency checks that are cheap enough for every CI run
// (`pnpm check:repo`). Each check prints what is wrong and how to fix it.
//
//  1. labeler-globs   every glob in .github/labeler.yml matches a tracked file (CI-31)
//  2. asset-names     release asset names in READMEs / the manual are ones
//                     release.yml actually publishes (CI-28, DOCS-30)
//  3. readme-parity   the four READMEs keep the same heading structure (DOCS-11)
//  4. stale-paths     docs do not point at paths that no longer exist
//                     (src/locales, root index.html) (CI-28)
//  5. tool-list       product descriptions name all supported tools (DOCS-23)
//  6. large-images    no new tracked image over 500 KB (CI-32)

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Every downloadable asset suffix the release workflow publishes, as in
 * `CC-Switch-v<version>-<suffix>`. Keep in sync with release.yml; the check
 * below verifies release.yml still produces each one.
 */
export const RELEASE_ASSET_SUFFIXES = [
  "macOS.dmg",
  "macOS.zip",
  "macOS.tar.gz",
  "Windows.msi",
  "Windows-Portable.zip",
  "Windows-arm64.msi",
  "Windows-arm64-Portable.zip",
  "Linux-x86_64.AppImage",
  "Linux-x86_64.deb",
  "Linux-x86_64.rpm",
  "Linux-arm64.AppImage",
  "Linux-arm64.deb",
  "Linux-arm64.rpm",
];

const README_FILES = [
  "README.md",
  "README_ZH.md",
  "README_JA.md",
  "README_DE.md",
];

// Images already over the limit when the guard was added. Shrink or move them
// to the website, then delete the entry; never add new ones (CI-32).
const LARGE_IMAGE_ALLOWLIST = new Set([
  "assets/partners/banners/kimi-banner-zh.png",
  "assets/partners/banners/kimi-banner-en.png",
  "assets/partners/banners/fable5.png",
  "assets/partners/logos/etok.png",
  "assets/partners/banners/minimax-en.jpeg",
  "assets/partners/banners/minimax-zh.jpeg",
  "assets/partners/logos/subrouter-banner.png",
  "docs/user-manual/assets/image-20260108010324583.png",
  // The macOS .icns bundles every resolution up to 1024x1024.
  "src-tauri/icons/icon.icns",
]);
const LARGE_IMAGE_BYTES = 500 * 1024;

function trackedFiles() {
  return execFileSync("git", ["ls-files", "-z"], {
    cwd: ROOT,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean);
}

/** Minimal glob -> RegExp for labeler/minimatch-style patterns. */
export function globToRegExp(glob) {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*") {
      if (glob[i + 1] === "*") {
        // "**/" matches zero or more directories; a trailing "**" anything.
        if (glob[i + 2] === "/") {
          re += "(?:.*/)?";
          i += 2;
        } else {
          re += ".*";
          i += 1;
        }
      } else {
        re += "[^/]*";
      }
    } else if (c === "?") {
      re += "[^/]";
    } else if ("\\^$+.()|{}[]".includes(c)) {
      re += `\\${c}`;
    } else {
      re += c;
    }
  }
  return new RegExp(`^${re}$`);
}

export function labelerGlobs(yamlText) {
  const globs = [];
  let label = null;
  for (const line of yamlText.split(/\r?\n/)) {
    const top = line.match(/^([A-Za-z0-9_.-]+):\s*$/);
    if (top) label = top[1];
    const item = line.match(/^\s+-\s+["']?([^"'#]+?)["']?\s*$/);
    if (item && label && !item[1].endsWith(":")) {
      globs.push({ label, glob: item[1] });
    }
  }
  return globs;
}

function checkLabelerGlobs(files, errors) {
  const text = fs.readFileSync(path.join(ROOT, ".github/labeler.yml"), "utf8");
  for (const { label, glob } of labelerGlobs(text)) {
    if (glob.startsWith("changed-files") || glob.startsWith("any-glob"))
      continue;
    const re = globToRegExp(glob);
    if (!files.some((f) => re.test(f))) {
      errors.push(
        `labeler-globs: "${glob}" (label "${label}") matches no tracked file; fix or remove it in .github/labeler.yml`,
      );
    }
  }
}

/** Asset suffixes mentioned in a doc, e.g. "Linux-x86_64.deb" or "Linux-*.deb". */
export function mentionedAssetSuffixes(text) {
  const out = [];
  const re = /CC-Switch-(?:v?\{[^}]+\}|v\d+\.\d+\.\d+|\*)-([A-Za-z0-9_.*-]+)/g;
  for (const m of text.matchAll(re)) {
    out.push(m[1].replace(/[.,;:]+$/, ""));
  }
  // "CC-Switch-*.AppImage" style (no separate version part).
  for (const m of text.matchAll(/CC-Switch-\*(\.[A-Za-z]+)\b/g))
    out.push(`*${m[1]}`);
  return out;
}

export function isKnownAssetSuffix(suffix) {
  if (suffix.includes("*")) {
    const re = globToRegExp(suffix);
    return RELEASE_ASSET_SUFFIXES.some((s) => re.test(s));
  }
  return RELEASE_ASSET_SUFFIXES.includes(suffix);
}

function checkAssetNames(files, errors) {
  const docs = files.filter(
    (f) =>
      (README_FILES.includes(f) ||
        f.startsWith("docs/user-manual/") ||
        f === "RELEASING.md") &&
      f.endsWith(".md"),
  );
  for (const f of docs) {
    const text = fs.readFileSync(path.join(ROOT, f), "utf8");
    for (const suffix of mentionedAssetSuffixes(text)) {
      if (!isKnownAssetSuffix(suffix)) {
        errors.push(
          `asset-names: ${f} mentions "CC-Switch-v{version}-${suffix}", which release.yml does not publish (known: ${RELEASE_ASSET_SUFFIXES.join(", ")})`,
        );
      }
    }
  }
  const release = fs.readFileSync(
    path.join(ROOT, ".github/workflows/release.yml"),
    "utf8",
  );
  // release.yml builds names from variables; check the distinctive fragments.
  const fragments = {
    "macOS.dmg": "-macOS.dmg",
    "macOS.zip": "-macOS.zip",
    "macOS.tar.gz": "-macOS.tar.gz",
    "Windows.msi": "-Windows$assetSuffix.msi",
    "Windows-Portable.zip": "-Windows$assetSuffix-Portable.zip",
    "Linux-x86_64.AppImage": "-Linux-${ARCH}.AppImage",
    "Linux-x86_64.deb": "-Linux-${ARCH}.deb",
    "Linux-x86_64.rpm": "-Linux-${ARCH}.rpm",
  };
  for (const [suffix, fragment] of Object.entries(fragments)) {
    if (!release.includes(fragment)) {
      errors.push(
        `asset-names: release.yml no longer contains "${fragment}" (asset ${suffix}); update RELEASE_ASSET_SUFFIXES in scripts/check-repo.mjs and the docs`,
      );
    }
  }
}

/** Heading levels outside fenced code blocks, e.g. [1, 3, 2, 2, 3]. */
export function headingLevels(markdown) {
  const levels = [];
  let fenced = false;
  for (const line of markdown.split(/\r?\n/)) {
    if (/^\s*```/.test(line)) fenced = !fenced;
    if (fenced) continue;
    const m = line.match(/^(#{1,6})\s/);
    if (m) levels.push(m[1].length);
  }
  return levels;
}

function checkReadmeParity(errors) {
  const shapes = README_FILES.map((f) => ({
    f,
    levels: headingLevels(fs.readFileSync(path.join(ROOT, f), "utf8")).join(""),
  }));
  const ref = shapes[0];
  for (const s of shapes.slice(1)) {
    if (s.levels !== ref.levels) {
      errors.push(
        `readme-parity: ${s.f} heading structure differs from ${ref.f} (${s.levels.length} vs ${ref.levels.length} headings); keep the four READMEs section-for-section in sync`,
      );
    }
  }
}

function checkStalePaths(files, errors) {
  const docs = files.filter(
    (f) =>
      f.endsWith(".md") &&
      !f.startsWith("docs/roadmap/") &&
      !f.startsWith("docs/release-notes/") &&
      f !== "CHANGELOG.md",
  );
  for (const f of docs) {
    const text = fs.readFileSync(path.join(ROOT, f), "utf8");
    if (/\bsrc\/locales\b/.test(text)) {
      errors.push(
        `stale-paths: ${f} mentions src/locales; translations live in src/i18n/locales/{en,zh,zh-TW,ja}.json`,
      );
    }
  }
}

function checkToolList(errors) {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(ROOT, "package.json"), "utf8"),
  );
  const surfaces = {
    "package.json description": pkg.description,
    "src-tauri/Cargo.toml description":
      fs
        .readFileSync(path.join(ROOT, "src-tauri/Cargo.toml"), "utf8")
        .match(/^description = "(.*)"$/m)?.[1] ?? "",
    "flatpak desktop Comment":
      fs
        .readFileSync(
          path.join(ROOT, "flatpak/com.ccswitch.desktop.desktop"),
          "utf8",
        )
        .match(/^Comment=(.*)$/m)?.[1] ?? "",
  };
  const tools = [
    "Claude Code",
    "Claude Desktop",
    "Codex",
    "Gemini CLI",
    "Grok Build",
    "OpenCode",
    "OpenClaw",
    "Hermes",
  ];
  for (const [where, text] of Object.entries(surfaces)) {
    const missing = tools.filter((t) => !text.includes(t));
    if (missing.length) {
      errors.push(`tool-list: ${where} does not mention ${missing.join(", ")}`);
    }
  }
}

function checkLargeImages(files, errors) {
  for (const f of files) {
    if (!/\.(png|jpe?g|gif|webp|icns|ico|bmp)$/i.test(f)) continue;
    if (LARGE_IMAGE_ALLOWLIST.has(f)) continue;
    let size;
    try {
      size = fs.statSync(path.join(ROOT, f)).size;
    } catch {
      continue;
    }
    if (size > LARGE_IMAGE_BYTES) {
      errors.push(
        `large-images: ${f} is ${(size / 1024).toFixed(0)} KB (limit ${LARGE_IMAGE_BYTES / 1024} KB); compress it (e.g. oxipng -o4 --strip safe) or host it elsewhere`,
      );
    }
  }
}

function main() {
  const files = trackedFiles();
  const errors = [];
  checkLabelerGlobs(files, errors);
  checkAssetNames(files, errors);
  checkReadmeParity(errors);
  checkStalePaths(files, errors);
  checkToolList(errors);
  checkLargeImages(files, errors);
  if (errors.length) {
    for (const e of errors) console.error(`✗ ${e}`);
    console.error(`\n${errors.length} repository check(s) failed.`);
    process.exit(1);
  }
  console.log(
    "✓ repository checks passed (labeler globs, asset names, README parity, stale paths, tool list, image sizes)",
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
