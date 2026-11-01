#!/usr/bin/env node
// Generates the sponsor table of the four READMEs from one data file (CI-33).
//
//   pnpm docs:sync           # rewrite the blocks from assets/partners/sponsors.json
//   pnpm docs:sync --check   # exit 1 when a README block differs (CI)
//   node scripts/sync-readme.mjs --extract   # rebuild the JSON from the READMEs
//
// Each README holds the table between `<!-- SPONSORS:START -->` and
// `<!-- SPONSORS:END -->`; edit assets/partners/sponsors.json instead of the
// READMEs. A sponsor's link, logo and alt text are shared unless a locale
// overrides them; the blurb (HTML) is per locale.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const DATA_FILE = "assets/partners/sponsors.json";
export const START = "<!-- SPONSORS:START -->";
export const END = "<!-- SPONSORS:END -->";
export const README_BY_LOCALE = {
  en: "README.md",
  zh: "README_ZH.md",
  ja: "README_JA.md",
  de: "README_DE.md",
};

const ROW_RE =
  /<tr>\n<td width="180"><a href="([^"]*)"><img src="([^"]*)" alt="([^"]*)" width="150"><\/a><\/td>\n<td>([\s\S]*?)<\/td>\n<\/tr>/g;

/** Parse the rows of a sponsor `<table>` into `{ href, logo, alt, blurb }`. */
export function parseTable(table) {
  const rows = [];
  for (const m of table.matchAll(ROW_RE)) {
    rows.push({ href: m[1], logo: m[2], alt: m[3], blurb: m[4] });
  }
  return rows;
}

/** Render one locale's table exactly as the READMEs lay it out. */
export function renderTable(data, locale) {
  const rows = data.sponsors.map((sponsor) => {
    const cell = { ...sponsor, ...(sponsor.overrides?.[locale] ?? {}) };
    const blurb = sponsor.blurb[locale];
    if (blurb === undefined) {
      throw new Error(`sponsor "${sponsor.alt}" has no ${locale} blurb`);
    }
    return (
      `<tr>\n<td width="180"><a href="${cell.href}"><img src="${cell.logo}" ` +
      `alt="${cell.alt}" width="150"></a></td>\n<td>${blurb}</td>\n</tr>`
    );
  });
  return `<table>\n${rows.join("\n\n")}\n\n</table>`;
}

/** The text between the markers, or null when a README has no markers. */
export function readBlock(markdown) {
  const start = markdown.indexOf(START);
  const end = markdown.indexOf(END);
  if (start < 0 || end < start) return null;
  return markdown.slice(start + START.length, end).replace(/^\n|\n$/g, "");
}

export function writeBlock(markdown, table) {
  const start = markdown.indexOf(START);
  const end = markdown.indexOf(END);
  if (start < 0 || end < start) {
    throw new Error(`missing ${START} / ${END} markers`);
  }
  return `${markdown.slice(0, start + START.length)}\n${table}\n${markdown.slice(end)}`;
}

/** Build the data file from the four README tables (used once, and to recover). */
export function extract(readmes) {
  const tables = Object.fromEntries(
    Object.entries(readmes).map(([locale, md]) => {
      const block = readBlock(md) ?? md.match(/<table>[\s\S]*?<\/table>/)?.[0];
      if (!block) throw new Error(`no sponsor table for ${locale}`);
      return [locale, parseTable(block)];
    }),
  );
  const base = tables.en;
  for (const [locale, rows] of Object.entries(tables)) {
    if (rows.length !== base.length) {
      throw new Error(
        `${locale} has ${rows.length} sponsors, en has ${base.length}; align the READMEs first`,
      );
    }
  }
  const sponsors = base.map((row, index) => {
    const sponsor = {
      alt: row.alt,
      href: row.href,
      logo: row.logo,
      blurb: {},
    };
    for (const [locale, rows] of Object.entries(tables)) {
      const other = rows[index];
      sponsor.blurb[locale] = other.blurb;
      const diff = {};
      for (const key of ["href", "logo", "alt"]) {
        if (other[key] !== row[key]) diff[key] = other[key];
      }
      if (Object.keys(diff).length) {
        sponsor.overrides ??= {};
        sponsor.overrides[locale] = diff;
      }
    }
    return sponsor;
  });
  return { sponsors };
}

function readReadmes(root) {
  return Object.fromEntries(
    Object.entries(README_BY_LOCALE).map(([locale, file]) => [
      locale,
      fs.readFileSync(path.join(root, file), "utf8"),
    ]),
  );
}

/** Files whose sponsor block differs from the data (empty when in sync). */
export function outOfSync(root = ROOT) {
  const data = JSON.parse(fs.readFileSync(path.join(root, DATA_FILE), "utf8"));
  const readmes = readReadmes(root);
  return Object.entries(readmes)
    .filter(([locale, md]) => readBlock(md) !== renderTable(data, locale))
    .map(([locale]) => README_BY_LOCALE[locale]);
}

function main(argv) {
  const readmes = readReadmes(ROOT);
  if (argv.includes("--extract")) {
    const data = extract(readmes);
    fs.writeFileSync(
      path.join(ROOT, DATA_FILE),
      `${JSON.stringify(data, null, 2)}\n`,
    );
    console.log(`wrote ${DATA_FILE} (${data.sponsors.length} sponsors)`);
    return 0;
  }
  if (argv.includes("--check")) {
    const stale = outOfSync(ROOT);
    if (stale.length) {
      console.error(
        `sponsor tables out of date in ${stale.join(", ")}: edit ${DATA_FILE} and run \`pnpm docs:sync\` (do not edit the README blocks by hand)`,
      );
      return 1;
    }
    console.log("✓ README sponsor tables match assets/partners/sponsors.json");
    return 0;
  }
  const data = JSON.parse(fs.readFileSync(path.join(ROOT, DATA_FILE), "utf8"));
  for (const [locale, md] of Object.entries(readmes)) {
    const next = writeBlock(md, renderTable(data, locale));
    if (next !== md) {
      fs.writeFileSync(path.join(ROOT, README_BY_LOCALE[locale]), next);
      console.log(`updated ${README_BY_LOCALE[locale]}`);
    }
  }
  return 0;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exit(main(process.argv.slice(2)));
}
