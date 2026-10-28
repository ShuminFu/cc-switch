import path from "node:path";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";
import {
  cargoPackageVersion,
  checkVersions,
  hasChangelogEntry,
  readVersions,
} from "../../scripts/check-versions.mjs";

// 发布前 tag 必须与 package.json / Cargo.toml / tauri.conf.json 的版本一致，
// 且 CHANGELOG.md 中有对应条目；CI 与 release.yml 都会运行该脚本（CCS-033）。
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/check-versions.mjs");

const CHANGELOG = "# Changelog\n\n## [1.2.3] - 2026-01-01\n\n- x\n";

function writeFixture(
  dir: string,
  {
    pkg = "1.2.3",
    cargo = "1.2.3",
    tauri = "1.2.3",
    changelog = CHANGELOG,
  }: { pkg?: string; cargo?: string; tauri?: string; changelog?: string } = {},
) {
  mkdirSync(path.join(dir, "src-tauri"), { recursive: true });
  writeFileSync(
    path.join(dir, "package.json"),
    JSON.stringify({ name: "x", version: pkg }),
  );
  // 依赖表中的 version 不能被误认为包版本
  writeFileSync(
    path.join(dir, "src-tauri/Cargo.toml"),
    [
      "[package]",
      'name = "x"',
      `version = "${cargo}"`,
      "",
      "[dependencies.serde]",
      'version = "9.9.9"',
      "",
    ].join("\n"),
  );
  writeFileSync(
    path.join(dir, "src-tauri/tauri.conf.json"),
    JSON.stringify({ productName: "x", version: tauri }),
  );
  writeFileSync(path.join(dir, "CHANGELOG.md"), changelog);
}

const run = (args: string[], env: NodeJS.ProcessEnv = {}) =>
  spawnSync(process.execPath, [SCRIPT, ...args], {
    encoding: "utf8",
    env: { ...process.env, GITHUB_REF: "", ...env },
  });

describe("check-versions", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "check-versions-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("reads the [package] version from Cargo.toml", () => {
    const text = '[workspace]\nversion = "0.0.1"\n[package]\nversion = "2.0.0"';
    expect(cargoPackageVersion(text)).toBe("2.0.0");
    expect(cargoPackageVersion('[dependencies]\nversion = "1"')).toBeNull();
  });

  it("accepts consistent versions with a matching tag", () => {
    writeFixture(dir);
    const { versions, changelog } = readVersions(dir);
    expect(checkVersions({ tag: "v1.2.3", versions, changelog })).toEqual([]);
    const res = run(["v1.2.3", "--root", dir]);
    expect(res.status).toBe(0);
    expect(res.stdout).toContain("1.2.3");
  });

  it("fails when the tag does not match the manifests", () => {
    writeFixture(dir);
    const res = run(["v1.2.4", "--root", dir]);
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("tag v1.2.4 does not match package.json");
  });

  it.each([
    ["package.json", { pkg: "1.2.4" }],
    ["src-tauri/Cargo.toml", { cargo: "1.2.4" }],
    ["src-tauri/tauri.conf.json", { tauri: "1.2.4" }],
  ])("fails when %s drifts from the others", (file, override) => {
    writeFixture(dir, override);
    const res = run(["--root", dir]);
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("versions differ");
    expect(res.stderr).toContain(`${file}=1.2.4`);
  });

  it("fails without a CHANGELOG entry for the version", () => {
    writeFixture(dir, { changelog: "# Changelog\n\n## [1.2.2] - x\n" });
    const res = run(["v1.2.3", "--root", dir]);
    expect(res.status).toBe(1);
    expect(res.stderr).toContain('no "## [1.2.3]" entry');
  });

  it("does not match a version that is only a prefix of an entry", () => {
    expect(hasChangelogEntry("## [1.2.30] - x", "1.2.3")).toBe(false);
    expect(hasChangelogEntry("## [1x2x3] - x", "1.2.3")).toBe(false);
  });

  it("lets a prerelease version use the Unreleased entry", () => {
    expect(hasChangelogEntry("## [Unreleased]\n", "1.3.0-rc.1")).toBe(true);
    expect(hasChangelogEntry("## [Unreleased]\n", "1.3.0")).toBe(false);
  });

  it("takes the tag from GITHUB_REF on a tag push", () => {
    writeFixture(dir);
    expect(
      run(["--root", dir], { GITHUB_REF: "refs/tags/v1.2.3" }).status,
    ).toBe(0);
    expect(
      run(["--root", dir], { GITHUB_REF: "refs/tags/v9.0.0" }).status,
    ).toBe(1);
    expect(run(["--root", dir], { GITHUB_REF: "refs/heads/main" }).status).toBe(
      0,
    );
  });

  it("passes for the repository's own tag and fails for another one", () => {
    const { versions } = readVersions(ROOT);
    const version = versions["package.json"];
    expect(run([`v${version}`]).status).toBe(0);
    expect(run([`v${version}.999`]).status).toBe(1);
  });
});
