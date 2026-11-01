#!/usr/bin/env node
// Copy provider icons from @lobehub/icons-static-svg into src/icons/extracted
// and print the entries to add to the hand-maintained index (CI-30).
//
//   pnpm icons:extract deepseek kimi           # copy deepseek.svg, kimi.svg
//   pnpm icons:extract --color minimax         # prefer minimax-color.svg
//   pnpm icons:extract --list | grep -i qwen   # search the upstream set
//
// src/icons/extracted/index.ts and metadata.ts are maintained by hand (they
// also hold raster sponsor logos that are not in the upstream set), so this
// script never rewrites them: after copying it prints the `icons` entry for
// index.ts and a `metadata.ts` stub to paste in.
//
// The old extract-icons.js / filter-icons.js were CommonJS under
// "type": "module", required a generate-icon-index.js that no longer existed,
// and regenerated (i.e. truncated) index.ts; they were removed.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function parseArgs(argv) {
  const args = {
    names: [],
    color: false,
    list: false,
    source: path.join(ROOT, "node_modules/@lobehub/icons-static-svg/icons"),
    out: path.join(ROOT, "src/icons/extracted"),
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--color") args.color = true;
    else if (a === "--list") args.list = true;
    else if (a === "--source") args.source = path.resolve(argv[++i]);
    else if (a === "--out") args.out = path.resolve(argv[++i]);
    else if (a.startsWith("--")) throw new Error(`unknown option ${a}`);
    else args.names.push(a);
  }
  return args;
}

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/;

export function extract({ names, color, source, out }) {
  const results = [];
  for (const raw of names) {
    const name = raw.toLowerCase();
    if (!NAME_RE.test(name)) {
      results.push({ name, ok: false, reason: "invalid icon name" });
      continue;
    }
    const candidates = color
      ? [`${name}-color.svg`, `${name}.svg`]
      : [`${name}.svg`, `${name}-color.svg`];
    const file = candidates
      .map((c) => path.join(source, c))
      .find((p) => fs.existsSync(p));
    if (!file) {
      results.push({
        name,
        ok: false,
        reason: `not found in ${path.relative(ROOT, source)}`,
      });
      continue;
    }
    fs.mkdirSync(out, { recursive: true });
    const target = path.join(out, `${name}.svg`);
    fs.copyFileSync(file, target);
    results.push({
      name,
      ok: true,
      from: path.basename(file),
      target,
      svg: fs.readFileSync(target, "utf8").trim(),
    });
  }
  return results;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.list) {
    for (const f of fs
      .readdirSync(args.source)
      .filter((f) => f.endsWith(".svg"))
      .sort()) {
      console.log(f.replace(/\.svg$/, ""));
    }
    return;
  }
  if (!args.names.length) {
    console.error(
      "usage: pnpm icons:extract [--color] <name> [...]   (see scripts/README.md)",
    );
    process.exit(2);
  }
  const results = extract(args);
  let failed = false;
  for (const r of results) {
    if (!r.ok) {
      failed = true;
      console.error(`✗ ${r.name}: ${r.reason}`);
      continue;
    }
    console.log(`✓ ${r.name}.svg (from ${r.from})`);
    console.log(
      `\n  // index.ts → icons\n  ${r.name}: \`${r.svg.replace(/`/g, "\\`").replace(/\$\{/g, "\\${")}\`,`,
    );
    console.log(
      `\n  // metadata.ts → iconMetadata\n  ${r.name}: {\n    name: "${r.name}",\n    displayName: "${r.name}",\n    category: "ai-provider",\n    keywords: ["${r.name}"],\n    defaultColor: "#6B7280",\n  },\n`,
    );
  }
  if (failed) process.exit(1);
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
