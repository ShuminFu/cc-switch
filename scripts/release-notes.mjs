#!/usr/bin/env node
// Builds release texts from the per-version notes in docs/release-notes/
// (CI-21, DOCS-22, DOCS-17):
//
//   node scripts/release-notes.mjs --tag v3.17.0 --body release-body.md
//       GitHub release body: English summary + highlights, links to the full
//       notes in every language, the download table and verification steps.
//   node scripts/release-notes.mjs --tag v3.17.0 --updater-notes notes.txt
//       Short plain-text notes for latest.json ("notes"), shown by the in-app
//       updater.
//   node scripts/release-notes.mjs --tag v3.17.0 --check
//       Exit 1 unless docs/release-notes/<tag>-{en,zh,ja}.md all exist.
//
// --allow-missing falls back to a generic text instead of failing when the
// English note is missing (dry runs on branches use it).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const NOTES_DIR = path.join(ROOT, "docs", "release-notes");
export const LOCALES = ["en", "zh", "ja"];
const REPO_URL = "https://github.com/farion1231/cc-switch";
// Upper bound for latest.json notes; the updater dialog is small.
export const UPDATER_NOTES_MAX = 1500;

export function notePath(tag, locale, dir = NOTES_DIR) {
  return path.join(dir, `${tag}-${locale}.md`);
}

/** Returns the body lines of the first `## <heading>` section, or null. */
export function extractSection(markdown, heading) {
  const lines = markdown.split(/\r?\n/);
  const start = lines.findIndex(
    (l) => l.trim().toLowerCase() === `## ${heading}`.toLowerCase(),
  );
  if (start === -1) return null;
  const out = [];
  for (const line of lines.slice(start + 1)) {
    if (/^#{1,2} /.test(line)) break;
    if (line.trim() === "---") break;
    out.push(line);
  }
  const text = out.join("\n").trim();
  return text || null;
}

/** The `> ...` summary paragraph right below the title, without the marker. */
export function extractSummary(markdown) {
  const lines = markdown.split(/\r?\n/);
  const out = [];
  let started = false;
  for (const line of lines) {
    if (!started) {
      if (line.startsWith("> ")) started = true;
      else continue;
    }
    if (!line.startsWith(">")) break;
    out.push(line.replace(/^>\s?/, ""));
  }
  const text = out.join(" ").trim();
  return text || null;
}

/** Markdown to compact plain text for the updater dialog. */
export function toPlainText(markdown) {
  return markdown
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^\s*[-*] /gm, "• ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function truncate(text, max) {
  if (text.length <= max) return text;
  const cut = text.slice(0, max - 1);
  const lastBreak = Math.max(cut.lastIndexOf("\n"), cut.lastIndexOf(". "));
  return `${(lastBreak > max / 2 ? cut.slice(0, lastBreak + 1) : cut).trimEnd()}…`;
}

export function updaterNotes(tag, markdown) {
  const url = `${REPO_URL}/releases/tag/${tag}`;
  if (!markdown) return `CC Switch ${tag}. Release notes: ${url}`;
  const source =
    extractSection(markdown, "Highlights") ?? extractSummary(markdown);
  if (!source) return `CC Switch ${tag}. Release notes: ${url}`;
  const suffix = `\n\nFull release notes: ${url}`;
  return (
    truncate(toPlainText(source), UPDATER_NOTES_MAX - suffix.length) + suffix
  );
}

export function downloadTable(tag) {
  const a = (name) => `\`CC-Switch-${tag}-${name}\``;
  return [
    "| Platform | Installer / 安装版 / インストーラー | Portable / 绿色版 / ポータブル |",
    "| --- | --- | --- |",
    `| macOS (Universal, signed + notarized) | ${a("macOS.dmg")} | ${a("macOS.zip")} |`,
    `| Windows x64 | ${a("Windows.msi")} | ${a("Windows-Portable.zip")} |`,
    `| Windows ARM64 | ${a("Windows-arm64.msi")} | ${a("Windows-arm64-Portable.zip")} |`,
    `| Linux x86_64 | ${a("Linux-x86_64.deb")} / ${a("Linux-x86_64.rpm")} | ${a("Linux-x86_64.AppImage")} |`,
    `| Linux ARM64 | ${a("Linux-arm64.deb")} / ${a("Linux-arm64.rpm")} | ${a("Linux-arm64.AppImage")} |`,
    "",
    "`.tar.gz` / `.sig` / `latest.json` are used by the in-app updater; you do not need to download them.",
  ].join("\n");
}

export function releaseBody(tag, notes, repo = "farion1231/cc-switch") {
  const blob = `${REPO_URL}/blob/${tag}/docs/release-notes`;
  const parts = [`## CC Switch ${tag}`, ""];
  parts.push(
    "🌐 **Only Official Website / 唯一官方网站 / 唯一の公式サイト**: [ccswitch.io](https://ccswitch.io)",
    "",
  );
  const en = notes.en;
  const summary = en ? extractSummary(en) : null;
  const highlights = en ? extractSection(en, "Highlights") : null;
  if (summary) parts.push(summary, "");
  if (highlights) parts.push("### Highlights", "", highlights, "");
  const links = LOCALES.filter((l) => notes[l]).map((l) => {
    const label = { en: "English", zh: "中文", ja: "日本語" }[l];
    return `[${label}](${blob}/${tag}-${l}.md)`;
  });
  if (links.length) {
    parts.push(
      `**Full release notes / 完整更新说明 / リリースノート**: ${links.join(" · ")}`,
      "",
    );
  }
  parts.push("### Downloads / 下载 / ダウンロード", "", downloadTable(tag), "");
  parts.push(
    "### Verify your download / 校验下载 / ダウンロードの検証",
    "",
    "```bash",
    "sha256sum -c SHA256SUMS --ignore-missing",
    `gh attestation verify CC-Switch-${tag}-Windows.msi --repo ${repo}`,
    "```",
    "",
    `See [SECURITY.md](${REPO_URL}/blob/main/SECURITY.md#verifying-a-download) for details.`,
  );
  return parts.join("\n").trimEnd() + "\n";
}

export function readNotes(tag, dir = NOTES_DIR) {
  const notes = {};
  for (const locale of LOCALES) {
    const p = notePath(tag, locale, dir);
    notes[locale] = fs.existsSync(p) ? fs.readFileSync(p, "utf8") : null;
  }
  return notes;
}

function parseArgs(argv) {
  const args = { allowMissing: false, check: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--tag") args.tag = argv[++i];
    else if (a === "--body") args.body = argv[++i];
    else if (a === "--updater-notes") args.updaterNotes = argv[++i];
    else if (a === "--allow-missing") args.allowMissing = true;
    else if (a === "--check") args.check = true;
    else if (a === "--repo") args.repo = argv[++i];
    else throw new Error(`unknown argument: ${a}`);
  }
  if (!args.tag || !/^v\d+\.\d+\.\d+/.test(args.tag)) {
    throw new Error("--tag vX.Y.Z is required");
  }
  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const notes = readNotes(args.tag);
  const missing = LOCALES.filter((l) => !notes[l]);
  if (args.check) {
    if (missing.length) {
      console.error(
        `Missing release notes for ${args.tag}: ${missing
          .map((l) => path.relative(ROOT, notePath(args.tag, l)))
          .join(", ")}`,
      );
      process.exit(1);
    }
    console.log(`Release notes for ${args.tag} exist in ${LOCALES.join(", ")}`);
    return;
  }
  if (!notes.en && !args.allowMissing) {
    console.error(
      `docs/release-notes/${args.tag}-en.md is missing. Write the release notes before tagging (RELEASING.md).`,
    );
    process.exit(1);
  }
  if (args.body) {
    fs.writeFileSync(args.body, releaseBody(args.tag, notes, args.repo));
    console.log(`Wrote ${args.body}`);
  }
  if (args.updaterNotes) {
    fs.writeFileSync(args.updaterNotes, updaterNotes(args.tag, notes.en));
    console.log(`Wrote ${args.updaterNotes}`);
  }
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main();
  } catch (e) {
    console.error(e.message);
    process.exit(2);
  }
}
