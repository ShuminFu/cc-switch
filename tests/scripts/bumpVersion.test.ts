import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const SCRIPT = path.resolve(__dirname, "../../scripts/bump-version.mjs");
const REPO = path.resolve(__dirname, "../..");

const dirs: string[] = [];

function fixture(versions: {
  pkg?: string;
  conf?: string;
  cargo?: string;
  lock?: string;
}) {
  const v = { pkg: "1.2.3", conf: "1.2.3", cargo: "1.2.3", lock: "1.2.3" };
  Object.assign(v, versions);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bump-version-"));
  dirs.push(dir);
  fs.mkdirSync(path.join(dir, "src-tauri"));
  fs.writeFileSync(
    path.join(dir, "package.json"),
    `{\n  "name": "cc-switch",\n  "version": "${v.pkg}",\n  "dependencies": {\n    "x": {\n      "version": "9.9.9"\n    }\n  }\n}\n`,
  );
  fs.writeFileSync(
    path.join(dir, "src-tauri/tauri.conf.json"),
    `{\n  "productName": "CC Switch",\n  "version": "${v.conf}",\n  "plugins": {}\n}\n`,
  );
  fs.writeFileSync(
    path.join(dir, "src-tauri/Cargo.toml"),
    `[package]\nname = "cc-switch"\nversion = "${v.cargo}"\nauthors = ["A"]\n\n[dependencies]\nserde = { version = "1.0" }\n`,
  );
  fs.writeFileSync(
    path.join(dir, "src-tauri/Cargo.lock"),
    `version = 4\n\n[[package]]\nname = "aaa"\nversion = "0.1.0"\n\n[[package]]\nname = "cc-switch"\nversion = "${v.lock}"\ndependencies = [\n "aaa",\n]\n`,
  );
  return dir;
}

function run(args: string[]) {
  const res = spawnSync(process.execPath, [SCRIPT, ...args], {
    encoding: "utf8",
  });
  return { code: res.status, out: `${res.stdout}${res.stderr}` };
}

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("scripts/bump-version.mjs", () => {
  it("the repository's four version files agree", () => {
    const res = run(["--check", "--root", REPO]);
    expect(res.out).toContain("is consistent across 4 files");
    expect(res.code).toBe(0);
  });

  it("passes --check when all files agree and the tag matches", () => {
    const dir = fixture({});
    const res = run(["--check", "--tag", "v1.2.3", "--root", dir]);
    expect(res.code).toBe(0);
    expect(res.out).toContain("matches tag v1.2.3");
  });

  it("fails --check when one file drifts", () => {
    const dir = fixture({ lock: "1.2.2" });
    const res = run(["--check", "--root", dir]);
    expect(res.code).toBe(1);
    expect(res.out).toContain("src-tauri/Cargo.lock has 1.2.2");
  });

  it("fails --check when the tag does not match", () => {
    const dir = fixture({});
    const res = run(["--check", "--tag", "refs/tags/v1.2.4", "--root", dir]);
    expect(res.code).toBe(1);
    expect(res.out).toContain("does not match the embedded version 1.2.3");
  });

  it("bumps all four files and leaves other version fields alone", () => {
    const dir = fixture({ lock: "1.0.0" });
    const res = run(["v1.3.0-rc.1", "--root", dir]);
    expect(res.code).toBe(0);
    expect(run(["--check", "--tag", "v1.3.0-rc.1", "--root", dir]).code).toBe(
      0,
    );
    const pkg = fs.readFileSync(path.join(dir, "package.json"), "utf8");
    expect(pkg).toContain('"version": "9.9.9"');
    const cargo = fs.readFileSync(
      path.join(dir, "src-tauri/Cargo.toml"),
      "utf8",
    );
    expect(cargo).toContain('serde = { version = "1.0" }');
    const lock = fs.readFileSync(
      path.join(dir, "src-tauri/Cargo.lock"),
      "utf8",
    );
    expect(lock).toContain('name = "aaa"\nversion = "0.1.0"');
  });

  it("rejects an invalid version without touching any file", () => {
    const dir = fixture({});
    const before = fs.readFileSync(path.join(dir, "package.json"), "utf8");
    const res = run(["1.2", "--root", dir]);
    expect(res.code).toBe(1);
    expect(res.out).toContain("Not a valid semver version");
    expect(fs.readFileSync(path.join(dir, "package.json"), "utf8")).toBe(
      before,
    );
  });
});
