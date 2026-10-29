#!/usr/bin/env node
// Count how many Tauri commands return `IpcError` (CCS-114).
//
// Usage:
//   node scripts/count-ipc-error-commands.mjs [--root <dir>] [--min <percent>]
//
// Scans every `#[tauri::command]` under src-tauri/src and checks whether the
// function's return type is `Result<_, IpcError>`. Prints a per-file table and
// the overall share; with `--min` it exits 1 when the share is below the given
// percentage (P2 target: 50, P4 target: 100).
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const COMMAND_ATTR = /#\[tauri::command[^\]]*\]/g;

/** Return the signature text of the fn that follows `start`, up to its body `{`. */
function signatureAfter(source, start) {
  const fnIdx = source.slice(start).search(/\bfn\s/);
  if (fnIdx < 0) return null;
  let depth = 0;
  for (let i = start + fnIdx; i < source.length; i += 1) {
    const c = source[i];
    if (c === "(" || c === "[" || c === "<") depth += 1;
    else if (c === ")" || c === "]") depth -= 1;
    else if (c === ">" && source[i - 1] !== "-") depth -= 1;
    else if ((c === "{" || c === ";") && depth === 0) {
      return source.slice(start + fnIdx, i);
    }
  }
  return null;
}

/** List the commands declared in one Rust source file. */
export function scanCommands(source) {
  const commands = [];
  for (const m of source.matchAll(COMMAND_ATTR)) {
    const sig = signatureAfter(source, m.index + m[0].length);
    if (!sig) continue;
    const name = sig.match(/\bfn\s+([A-Za-z0-9_]+)/)?.[1] ?? "?";
    const ipcError = /,\s*(?:crate::error::)?IpcError\s*>\s*$/.test(sig.trim());
    commands.push({ name, ipcError });
  }
  return commands;
}

/** Aggregate per-file results: `files` maps a path to its Rust source. */
export function countIpcErrorCommands(files) {
  const perFile = [];
  let total = 0;
  let ipc = 0;
  for (const [file, source] of Object.entries(files)) {
    const commands = scanCommands(source);
    if (commands.length === 0) continue;
    const migrated = commands.filter((c) => c.ipcError).length;
    perFile.push({ file, total: commands.length, ipcError: migrated });
    total += commands.length;
    ipc += migrated;
  }
  perFile.sort((a, b) => a.file.localeCompare(b.file));
  const percent = total === 0 ? 0 : (ipc / total) * 100;
  return { total, ipcError: ipc, percent, perFile };
}

function walkRust(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkRust(full, out);
    else if (entry.name.endsWith(".rs")) out.push(full);
  }
  return out;
}

/** Read every .rs file under `<root>/src-tauri/src`. */
export function readCommandSources(root) {
  const base = path.join(root, "src-tauri/src");
  const files = {};
  for (const full of walkRust(base)) {
    files[path.relative(root, full)] = readFileSync(full, "utf8");
  }
  return files;
}

function main(argv) {
  let root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  let min = null;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--root") root = path.resolve(argv[++i]);
    else if (argv[i] === "--min") min = Number(argv[++i]);
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  if (min !== null && !Number.isFinite(min)) {
    throw new Error("--min expects a number");
  }

  const result = countIpcErrorCommands(readCommandSources(root));
  for (const f of result.perFile) {
    console.log(`${String(f.ipcError).padStart(3)}/${String(f.total).padEnd(3)} ${f.file}`);
  }
  console.log(
    `IpcError commands: ${result.ipcError}/${result.total} (${result.percent.toFixed(1)}%)`,
  );
  if (min !== null && result.percent < min) {
    console.error(`❌ below the ${min}% target`);
    return 1;
  }
  return 0;
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
