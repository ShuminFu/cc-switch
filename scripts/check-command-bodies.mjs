#!/usr/bin/env node
// Keep Tauri commands thin adapters (CCS-181).
//
// Usage:
//   node scripts/check-command-bodies.mjs [--root <dir>] [--max-lines <n>]
//
// Scans every `#[tauri::command]` under src-tauri/src and fails when a command
// body is longer than `--max-lines` (default 40) or queries the database
// directly. A command may hand the database to a service (`&state.db`,
// `state.db.clone()`), but calling a method on it (`state.db.get_x()`,
// `db.save_y()`) or taking its connection (`lock_conn!`) belongs in a service.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULT_MAX_LINES = 40;

const COMMAND_ATTR = /#\[tauri::command[^\]]*\]/g;
// 对 `db` 的任何成员访问（clone / as_ref 除外，那只是把句柄交给 service）
const DB_ACCESS = /\bdb\s*\.\s*(?!(?:clone|as_ref)\s*\()[A-Za-z_]\w*/;
const LOCK_CONN = /\block_conn!/;

/**
 * Blank out comments, string and char literals so braces and keywords inside
 * them do not confuse the scanner. Newlines are kept, so offsets and line
 * counts stay the same.
 */
export function stripLiterals(source) {
  const out = source.split("");
  const blank = (from, to) => {
    for (let k = from; k < to; k += 1) {
      if (out[k] !== "\n") out[k] = " ";
    }
  };
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    const next = source[i + 1];
    if (c === "/" && next === "/") {
      const end = source.indexOf("\n", i);
      const stop = end < 0 ? source.length : end;
      blank(i, stop);
      i = stop;
    } else if (c === "/" && next === "*") {
      let depth = 1;
      let j = i + 2;
      while (j < source.length && depth > 0) {
        if (source[j] === "/" && source[j + 1] === "*") {
          depth += 1;
          j += 2;
        } else if (source[j] === "*" && source[j + 1] === "/") {
          depth -= 1;
          j += 2;
        } else {
          j += 1;
        }
      }
      blank(i, j);
      i = j;
    } else if (
      (c === "r" || (c === "b" && next === "r")) &&
      /^b?r#*"/.test(source.slice(i, i + 260)) &&
      !/[A-Za-z0-9_]/.test(source[i - 1] ?? "")
    ) {
      const head = source.slice(i).match(/^b?r(#*)"/);
      const closing = `"${head[1]}`;
      const end = source.indexOf(closing, i + head[0].length);
      const stop = end < 0 ? source.length : end + closing.length;
      blank(i + head[0].length, stop - closing.length);
      i = stop;
    } else if (c === '"') {
      let j = i + 1;
      while (j < source.length && source[j] !== '"') {
        j += source[j] === "\\" ? 2 : 1;
      }
      blank(i + 1, j);
      i = j + 1;
    } else if (c === "'") {
      // 字符字面量：'x'、'\n'、'\u{1F600}'；否则是生命周期标注
      const m = source
        .slice(i)
        .match(/^'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'/);
      if (m) {
        blank(i + 1, i + m[0].length - 1);
        i += m[0].length;
      } else {
        i += 1;
      }
    } else {
      i += 1;
    }
  }
  return out.join("");
}

/** Index of the `{` that opens the body of the fn found after `start`. */
function bodyStart(code, start) {
  const fnIdx = code.slice(start).search(/\bfn\s/);
  if (fnIdx < 0) return -1;
  let depth = 0;
  for (let i = start + fnIdx; i < code.length; i += 1) {
    const c = code[i];
    if (c === "(" || c === "[" || c === "<") depth += 1;
    else if (c === ")" || c === "]") depth -= 1;
    else if (c === ">" && code[i - 1] !== "-") depth -= 1;
    else if (c === "{" && depth === 0) return i;
    else if (c === ";" && depth === 0) return -1;
  }
  return -1;
}

/** Index of the `}` matching the `{` at `open`. */
function matchingBrace(code, open) {
  let depth = 0;
  for (let i = open; i < code.length; i += 1) {
    if (code[i] === "{") depth += 1;
    else if (code[i] === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** List the commands of one Rust source file with their body size and DB use. */
export function scanCommandBodies(source) {
  const code = stripLiterals(source);
  const commands = [];
  for (const m of code.matchAll(COMMAND_ATTR)) {
    const open = bodyStart(code, m.index + m[0].length);
    if (open < 0) continue;
    const close = matchingBrace(code, open);
    if (close < 0) continue;
    const name =
      code.slice(m.index).match(/\bfn\s+([A-Za-z0-9_]+)/)?.[1] ?? "?";
    const body = code.slice(open + 1, close);
    // 只数 `{` 与 `}` 之间的行（单行函数体为 0 行）
    const lines = Math.max(0, body.split("\n").length - 2);
    const line = code.slice(0, m.index).split("\n").length;
    const dbAccess = DB_ACCESS.test(body) || LOCK_CONN.test(body);
    commands.push({ name, line, lines, dbAccess });
  }
  return commands;
}

/** Check every file: `files` maps a path to its Rust source. */
export function checkCommandBodies(files, maxLines = DEFAULT_MAX_LINES) {
  const violations = [];
  let total = 0;
  for (const [file, source] of Object.entries(files)) {
    for (const c of scanCommandBodies(source)) {
      total += 1;
      if (c.lines > maxLines) {
        violations.push({
          file,
          line: c.line,
          name: c.name,
          reason: `body is ${c.lines} lines (max ${maxLines})`,
        });
      }
      if (c.dbAccess) {
        violations.push({
          file,
          line: c.line,
          name: c.name,
          reason: "queries the database directly (move it into a service)",
        });
      }
    }
  }
  violations.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  return { total, violations };
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
  let maxLines = DEFAULT_MAX_LINES;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--root") root = path.resolve(argv[++i]);
    else if (argv[i] === "--max-lines") maxLines = Number(argv[++i]);
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  if (!Number.isInteger(maxLines) || maxLines < 1) {
    throw new Error("--max-lines expects a positive integer");
  }

  const { total, violations } = checkCommandBodies(
    readCommandSources(root),
    maxLines,
  );
  for (const v of violations) {
    console.error(`${v.file}:${v.line} ${v.name}: ${v.reason}`);
  }
  if (violations.length > 0) {
    console.error(
      `❌ ${violations.length} command body violation(s) in ${total} commands`,
    );
    return 1;
  }
  console.log(
    `✅ ${total} commands are thin adapters (≤ ${maxLines} lines, no direct DB access)`,
  );
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
