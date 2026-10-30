/**
 * Contract test (TEST-05): every Tauri command the frontend invokes must be
 * registered in `src-tauri/src/lib.rs` (`tauri::generate_handler![...]`), and
 * every MSW mock must target a registered command. A renamed or removed Rust
 * command otherwise only fails at runtime in the real app.
 *
 * This is a string-level check (no codegen): it parses the handler list and
 * scans `invoke("name")` call sites.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(__dirname, "../..");

function readRegisteredCommands(): Set<string> {
  const lib = fs.readFileSync(path.join(ROOT, "src-tauri/src/lib.rs"), "utf8");
  const start = lib.indexOf("tauri::generate_handler![");
  expect(start, "generate_handler! not found in lib.rs").toBeGreaterThan(-1);
  const end = lib.indexOf("])", start);
  const body = lib
    .slice(start + "tauri::generate_handler![".length, end)
    .replace(/\/\/[^\n]*/g, "")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/#\[[^\]]*\]/g, "");
  const names = body
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => entry.split("::").pop()!.trim());
  return new Set(names);
}

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (
      /\.(ts|tsx)$/.test(entry.name) &&
      !/\.test\.tsx?$/.test(entry.name)
    ) {
      out.push(full);
    }
  }
  return out;
}

// `invoke("cmd")`, `invoke<T>("cmd")`, `invoke<Array<T>>(\n "cmd", ...)`.
const INVOKE_RE = /\binvoke\s*(?:<[\s\S]*?>)?\(\s*["'`]([A-Za-z0-9_]+)["'`]/g;

function readInvokedCommands(): Map<string, string[]> {
  const invoked = new Map<string, string[]>();
  for (const file of listSourceFiles(path.join(ROOT, "src"))) {
    const text = fs.readFileSync(file, "utf8");
    for (const match of text.matchAll(INVOKE_RE)) {
      const rel = path.relative(ROOT, file);
      const files = invoked.get(match[1]) ?? [];
      if (!files.includes(rel)) files.push(rel);
      invoked.set(match[1], files);
    }
  }
  return invoked;
}

function readMswHandlerCommands(): string[] {
  const text = fs.readFileSync(
    path.join(ROOT, "tests/msw/handlers.ts"),
    "utf8",
  );
  return [...text.matchAll(/\$\{TAURI_ENDPOINT\}\/([A-Za-z0-9_]+)`/g)].map(
    (m) => m[1],
  );
}

describe("Tauri command registry contract", () => {
  const registered = readRegisteredCommands();
  const invoked = readInvokedCommands();

  it("parses a plausible number of registered commands", () => {
    expect(registered.size).toBeGreaterThan(200);
    expect(registered.has("switch_provider")).toBe(true);
    expect(registered.has("save_settings")).toBe(true);
  });

  it("finds the frontend invoke call sites", () => {
    expect(invoked.size).toBeGreaterThan(200);
    // Sanity: the API wrappers are covered by the scan.
    expect(
      invoked.get("switch_provider")?.some((f) => f.startsWith("src/lib/api/")),
    ).toBe(true);
  });

  it("every command invoked from src/ is registered in generate_handler!", () => {
    const missing = [...invoked.entries()]
      .filter(([name]) => !registered.has(name))
      .map(([name, files]) => `${name} (invoked from ${files.join(", ")})`)
      .sort();
    expect(missing).toEqual([]);
  });

  it("every MSW handler mocks a registered command", () => {
    const handlers = readMswHandlerCommands();
    expect(handlers.length).toBeGreaterThan(0);
    const phantom = handlers.filter((name) => !registered.has(name)).sort();
    expect(phantom).toEqual([]);
  });
});
