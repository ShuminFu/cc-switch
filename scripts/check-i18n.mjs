#!/usr/bin/env node
// Locale integrity check (I18N-05).
//
// Compares every locale in src/i18n/locales against the reference locale
// (en.json) and fails when:
//   - a key exists in en.json but is missing from another locale,
//   - a locale has a key that en.json does not have,
//   - a key's value type differs (string vs. nested object),
//   - the set of {{interpolation}} placeholders of a string differs.
//
// i18next plural suffixes (_zero/_one/_two/_few/_many/_other) are folded into
// their base key, because languages legitimately use different plural forms.
//
// Usage: node scripts/check-i18n.mjs [localesDir]

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const REFERENCE = "en";
const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;
const PLACEHOLDER = /\{\{\s*([^}]+?)\s*\}\}/g;

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const localesDir = path.resolve(
  process.argv[2] ?? path.join(repoRoot, "src/i18n/locales"),
);

/** Flatten a nested locale object into Map<key, string | "[object]">. */
function flatten(obj, prefix = "", out = new Map()) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      flatten(v, key, out);
    } else {
      out.set(key, v);
    }
  }
  return out;
}

/** Collapse plural variants into the base key, merging their placeholders. */
function normalize(flat) {
  const out = new Map();
  for (const [key, value] of flat) {
    const base = key.replace(PLURAL_SUFFIX, "");
    const entry = out.get(base) ?? {
      type: typeof value,
      placeholders: new Set(),
    };
    if (typeof value === "string") {
      for (const m of value.matchAll(PLACEHOLDER)) {
        // "{{count, number}}" -> "count"; "{{- name}}" -> "name"
        entry.placeholders.add(m[1].split(",")[0].replace(/^-\s*/, "").trim());
      }
    } else if (entry.type !== typeof value) {
      entry.type = "mixed";
    }
    out.set(base, entry);
  }
  return out;
}

function load(file) {
  const raw = fs.readFileSync(path.join(localesDir, file), "utf8");
  try {
    return normalize(flatten(JSON.parse(raw)));
  } catch (err) {
    throw new Error(`${file}: invalid JSON (${err.message})`);
  }
}

const files = fs
  .readdirSync(localesDir)
  .filter((f) => f.endsWith(".json"))
  .sort();
const refFile = `${REFERENCE}.json`;
if (!files.includes(refFile)) {
  console.error(
    `check-i18n: reference locale ${refFile} not found in ${localesDir}`,
  );
  process.exit(2);
}

const ref = load(refFile);
const problems = [];

for (const file of files) {
  if (file === refFile) continue;
  const loc = load(file);
  const missing = [...ref.keys()].filter((k) => !loc.has(k));
  const extra = [...loc.keys()].filter((k) => !ref.has(k));
  const typeMismatch = [];
  const placeholderMismatch = [];

  for (const [key, refEntry] of ref) {
    const entry = loc.get(key);
    if (!entry) continue;
    if (entry.type !== refEntry.type) {
      typeMismatch.push(
        `${key} (${REFERENCE}: ${refEntry.type}, ${file}: ${entry.type})`,
      );
      continue;
    }
    const a = [...refEntry.placeholders].sort().join(", ");
    const b = [...entry.placeholders].sort().join(", ");
    if (a !== b) {
      placeholderMismatch.push(`${key}: ${REFERENCE} {${a}} vs {${b}}`);
    }
  }

  const sections = [
    ["missing keys (present in en.json)", missing],
    ["extra keys (absent from en.json)", extra],
    ["value type mismatches", typeMismatch],
    ["placeholder mismatches", placeholderMismatch],
  ];
  for (const [label, list] of sections) {
    if (list.length === 0) continue;
    problems.push(`${file}: ${list.length} ${label}`);
    for (const item of list) problems.push(`  - ${item}`);
  }
}

if (problems.length > 0) {
  console.error("check-i18n: locale files are out of sync\n");
  console.error(problems.join("\n"));
  process.exit(1);
}

console.log(
  `check-i18n: ${files.length} locales OK (${ref.size} keys, reference ${refFile})`,
);
