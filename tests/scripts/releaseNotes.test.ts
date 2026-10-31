import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const SCRIPT = path.resolve(__dirname, "../../scripts/release-notes.mjs");
const dirs: string[] = [];

function tmp() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "release-notes-"));
  dirs.push(dir);
  return dir;
}

function run(args: string[]) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

afterEach(() => {
  for (const d of dirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

describe("scripts/release-notes.mjs", () => {
  it("builds an English release body with language links and downloads", () => {
    const out = path.join(tmp(), "body.md");
    const res = run(["--tag", "v3.17.0", "--body", out]);
    expect(res.status).toBe(0);
    const body = fs.readFileSync(out, "utf8");
    expect(body).toContain("## CC Switch v3.17.0");
    expect(body).toContain("### Highlights");
    expect(body).toContain("v3.17.0-zh.md");
    expect(body).toContain("v3.17.0-ja.md");
    expect(body).toContain("CC-Switch-v3.17.0-Windows-arm64.msi");
    expect(body).toContain("CC-Switch-v3.17.0-Linux-x86_64.AppImage");
    expect(body).toContain("sha256sum -c SHA256SUMS");
    // The old body was Chinese-only; the generated one is English-first.
    expect(body).not.toContain("Claude Code 供应商切换工具");
  });

  it("writes short, non-empty plain-text updater notes", () => {
    const out = path.join(tmp(), "notes.txt");
    const res = run(["--tag", "v3.17.0", "--updater-notes", out]);
    expect(res.status).toBe(0);
    const notes = fs.readFileSync(out, "utf8");
    expect(notes.length).toBeGreaterThan(50);
    expect(notes.length).toBeLessThanOrEqual(1500);
    expect(notes).not.toMatch(/\*\*|\]\(/);
    expect(notes).toContain("releases/tag/v3.17.0");
  });

  it("fails when the English note for the tag is missing", () => {
    const res = run(["--tag", "v0.0.1", "--body", path.join(tmp(), "b.md")]);
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("v0.0.1-en.md is missing");
  });

  it("falls back to a generic text with --allow-missing", () => {
    const out = path.join(tmp(), "notes.txt");
    const res = run(["--tag", "v0.0.1", "--allow-missing", "--updater-notes", out]);
    expect(res.status).toBe(0);
    expect(fs.readFileSync(out, "utf8")).toContain("CC Switch v0.0.1");
  });

  it("--check requires all three locales", () => {
    expect(run(["--tag", "v3.17.0", "--check"]).status).toBe(0);
    expect(run(["--tag", "v0.0.1", "--check"]).status).toBe(1);
  });
});
