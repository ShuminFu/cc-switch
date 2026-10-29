#!/usr/bin/env node
// Check the IPC command surface against the frontend (CCS-118).
//
// Usage:
//   node scripts/check-ipc-orphans.mjs [--root <dir>]
//
// Reads the `tauri::generate_handler![...]` list in src-tauri/src/lib.rs and
// every non-test .ts/.tsx file under src/ (the generated src/shared/bindings.ts
// is skipped, it lists every command). Reports:
//   - orphans: registered commands no frontend source names as a string literal
//   - unknown: `invoke("name")` calls for commands that are not registered
//   - badNames: registered command names that are not snake_case
// Exits 1 when any list is non-empty.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HANDLER_START = "tauri::generate_handler![";
const SNAKE_CASE = /^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/;
const INVOKE_CALL =
  /\binvoke\s*(?:<[\s\S]*?>)?\s*\(\s*(["'`])([A-Za-z0-9_]+)\1/g;
const STRING_LITERAL = /(["'`])([A-Za-z0-9_]+)\1/g;

/** Command names in the `generate_handler!` list, in registration order. */
export function registeredCommands(libSource) {
  const start = libSource.indexOf(HANDLER_START);
  if (start < 0) throw new Error("generate_handler! not found in lib.rs");
  const body = libSource.slice(start + HANDLER_START.length);
  const end = body.indexOf("]");
  if (end < 0) throw new Error("unterminated generate_handler! list");
  return body
    .slice(0, end)
    .split("\n")
    .map((line) => line.split("//")[0])
    .join(",")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => entry.split("::").pop());
}

/** Compare the registered commands with the frontend sources (path -> text). */
export function checkIpcCommands(libSource, frontendFiles) {
  const commands = registeredCommands(libSource);
  const registered = new Set(commands);
  const literals = new Set();
  const unknown = new Set();
  for (const source of Object.values(frontendFiles)) {
    for (const m of source.matchAll(STRING_LITERAL)) literals.add(m[2]);
    for (const m of source.matchAll(INVOKE_CALL)) {
      if (!registered.has(m[2])) unknown.add(m[2]);
    }
  }
  return {
    total: commands.length,
    orphans: commands.filter((name) => !literals.has(name)),
    unknown: [...unknown].sort(),
    badNames: commands.filter((name) => !SNAKE_CASE.test(name)),
  };
}

function walkFrontend(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkFrontend(full, out);
    else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

/** Read lib.rs and the non-test frontend sources under `<root>`. */
export function readSources(root) {
  const libSource = readFileSync(
    path.join(root, "src-tauri/src/lib.rs"),
    "utf8",
  );
  const bindings = path.join(root, "src/shared/bindings.ts");
  const frontendFiles = {};
  for (const full of walkFrontend(path.join(root, "src"))) {
    if (full === bindings) continue;
    frontendFiles[path.relative(root, full)] = readFileSync(full, "utf8");
  }
  return { libSource, frontendFiles };
}

function main(argv) {
  let root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--root") root = path.resolve(argv[++i]);
    else throw new Error(`unknown argument: ${argv[i]}`);
  }

  const { libSource, frontendFiles } = readSources(root);
  const result = checkIpcCommands(libSource, frontendFiles);
  const sections = [
    ["orphan commands (registered, never referenced by src/)", result.orphans],
    ["unknown commands (invoked, not registered)", result.unknown],
    ["non-snake_case command names", result.badNames],
  ];
  for (const [title, names] of sections) {
    if (names.length === 0) continue;
    console.error(`${title}:`);
    for (const name of names) console.error(`  - ${name}`);
  }
  console.log(
    `IPC commands: ${result.total} registered, ${result.orphans.length} orphans, ` +
      `${result.unknown.length} unknown, ${result.badNames.length} non-snake_case`,
  );
  return sections.some(([, names]) => names.length > 0) ? 1 : 0;
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
