import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";

// package.json 与 src-tauri/Cargo.toml 的 description 必须一致，
// 且不能退回只列出 Claude Code / Codex / Gemini CLI 的旧文案（CCS-047）。
const ROOT = path.resolve(__dirname, "../..");

const pkg = JSON.parse(
  readFileSync(path.join(ROOT, "package.json"), "utf8"),
) as {
  description?: string;
};
const cargoToml = readFileSync(path.join(ROOT, "src-tauri/Cargo.toml"), "utf8");
const cargoPackage = cargoToml.split(/^\[(?!package\])/m)[0];
const cargoDescription = cargoPackage.match(
  /^description\s*=\s*"([^"]*)"/m,
)?.[1];

describe("package description", () => {
  it("is identical in package.json and Cargo.toml", () => {
    expect(pkg.description).toBeTruthy();
    expect(cargoDescription).toBe(pkg.description);
  });

  it("does not use the stale three-tool wording", () => {
    for (const file of [
      "package.json",
      "src-tauri/Cargo.toml",
      "src-tauri/tauri.conf.json",
    ]) {
      const text = readFileSync(path.join(ROOT, file), "utf8");
      expect(text, file).not.toContain("Codex & Gemini CLI");
    }
  });

  it("mentions OpenCode alongside the original tools", () => {
    for (const tool of ["Claude Code", "Codex", "Gemini CLI", "OpenCode"]) {
      expect(pkg.description).toContain(tool);
    }
  });
});
