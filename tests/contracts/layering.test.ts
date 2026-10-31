import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Dependency direction of the frontend layers (FE-ARCH-29):
 * `config`/`types`/`utils`/`lib` never import UI layers (`components`,
 * `hooks`, `routes`, `app`), `lib/api` never imports UI libraries, and
 * `src/config` holds data only (no `.tsx`).
 */
const SRC = join(__dirname, "..", "..", "src");

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return walk(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

const IMPORT_RE = /(?:import|export)[^'"]*?from\s+["']([^"']+)["']/g;

function importsOf(file: string): string[] {
  const text = readFileSync(file, "utf8");
  return [...text.matchAll(IMPORT_RE)].map((m) => m[1]);
}

function violations(dirs: string[], forbidden: RegExp): string[] {
  return dirs
    .flatMap((dir) => walk(join(SRC, dir)))
    .filter((file) => !file.endsWith("bindings.ts"))
    .flatMap((file) =>
      importsOf(file)
        .filter((spec) => forbidden.test(spec))
        .map((spec) => `${relative(SRC, file)} -> ${spec}`),
    );
}

describe("frontend layering", () => {
  it("config, types, utils and lib do not import UI layers", () => {
    expect(
      violations(
        ["config", "types", "utils", "lib"],
        /^@\/(components|hooks|routes|app)(\/|$)/,
      ),
    ).toEqual([]);
  });

  it("the API layer does not import toast or i18n libraries", () => {
    expect(violations(["lib/api"], /^(sonner|i18next|react-i18next)$/)).toEqual(
      [],
    );
  });

  it("src/config contains no .tsx files", () => {
    const tsx = walk(join(SRC, "config")).filter((f) => f.endsWith(".tsx"));
    expect(tsx.map((f) => relative(SRC, f))).toEqual([]);
  });
});
