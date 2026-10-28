import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  existsSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";

// 发布卫生（CCS-033）：Cargo 缓存统一使用 Swatinem/rust-cache；CI 与 release.yml 都运行
// scripts/check-versions.mjs；发布产物附带 SHA256SUMS 与构建来源证明（provenance attestation）。
const ROOT = path.resolve(__dirname, "../..");
const read = (rel: string) => readFileSync(path.join(ROOT, rel), "utf8");
const ci = read(".github/workflows/ci.yml");
const release = read(".github/workflows/release.yml");
const SCRIPT = path.join(ROOT, "scripts/generate-sha256sums.sh");

/** 返回 jobs 下某个 job 的文本块 */
function jobBlock(text: string, job: string): string {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => l === `  ${job}:`);
  expect(start).toBeGreaterThan(-1);
  const end = lines.findIndex((l, i) => i > start && /^ {2}[\w-]+\s*:/.test(l));
  return lines.slice(start, end === -1 ? undefined : end).join("\n");
}

describe("release hygiene in workflows", () => {
  it.each([
    ["ci.yml", ci],
    ["release.yml", release],
  ])(
    "%s caches Cargo with rust-cache, not a manual actions/cache",
    (_, text) => {
      expect(text).not.toMatch(/~\/\.cargo\/registry/);
      expect(text).not.toMatch(/-cargo-\$\{\{\s*hashFiles/);
      expect(text).toMatch(/uses:\s*Swatinem\/rust-cache@/);
    },
  );

  it("caches Cargo in every ci.yml job that builds Rust", () => {
    for (const job of [
      "backend",
      "msrv",
      "tauri-build-smoke",
      "coverage-rust",
    ]) {
      expect(jobBlock(ci, job)).toMatch(/uses:\s*Swatinem\/rust-cache@/);
    }
  });

  it("caches Cargo in the release build matrix", () => {
    const block = jobBlock(release, "release");
    expect(block).toMatch(/uses:\s*Swatinem\/rust-cache@/);
    // 每个矩阵项使用独立的缓存
    expect(block).toMatch(/key:\s*release-\$\{\{ matrix\.os \}\}/);
  });

  it("ci.yml runs the version check", () => {
    expect(jobBlock(ci, "frontend")).toContain(
      "node scripts/check-versions.mjs",
    );
  });

  it("release builds wait for the tag/version check", () => {
    expect(jobBlock(release, "check-versions")).toContain(
      'node scripts/check-versions.mjs "$GITHUB_REF_NAME"',
    );
    expect(jobBlock(release, "release")).toMatch(
      /^ {4}needs:\s*\[ci, check-versions\]\s*$/m,
    );
  });

  it("publish-release writes SHA256SUMS and attests provenance before upload", () => {
    const block = jobBlock(release, "publish-release");
    expect(block).toMatch(/^ {6}id-token:\s*write\s*$/m);
    expect(block).toMatch(/^ {6}attestations:\s*write\s*$/m);
    const sums = block.indexOf("generate-sha256sums.sh release-assets");
    const attest = block.search(/uses:\s*actions\/attest-build-provenance@/);
    const upload = block.search(/uses:\s*softprops\/action-gh-release@/);
    expect(sums).toBeGreaterThan(-1);
    expect(attest).toBeGreaterThan(sums);
    expect(upload).toBeGreaterThan(attest);
    expect(block).toContain("subject-checksums: release-assets/SHA256SUMS");
    // SHA256SUMS 位于 release-assets/ 内，随 files: release-assets/* 一起上传
    expect(block).toContain("files: release-assets/*");
  });

  it("only publish-release gets OIDC / attestation write access", () => {
    const others = release
      .split(/\r?\n/)
      .filter((l) => /^\s+(id-token|attestations):\s*write/.test(l));
    expect(others).toHaveLength(2);
    expect(ci).not.toMatch(/id-token|attestations/);
  });
});

const canRun =
  spawnSync("bash", ["--version"], { stdio: "ignore" }).status === 0 &&
  spawnSync("sha256sum", ["--version"], { stdio: "ignore" }).status === 0;

describe.skipIf(!canRun)("generate-sha256sums.sh", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "sha256sums-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  const run = (...args: string[]) =>
    spawnSync("bash", [SCRIPT, ...args], { encoding: "utf8" });

  it("lists every asset by bare name and verifies with sha256sum -c", () => {
    writeFileSync(path.join(dir, "b.msi"), "bbb");
    writeFileSync(path.join(dir, "a.AppImage"), "aaa");
    writeFileSync(path.join(dir, "a.AppImage.sig"), "sig");
    // 旧的 SHA256SUMS 不能把自身列入
    writeFileSync(path.join(dir, "SHA256SUMS"), "stale");
    const res = run(dir);
    expect(res.status).toBe(0);
    const sums = readFileSync(path.join(dir, "SHA256SUMS"), "utf8");
    const names = sums
      .trim()
      .split("\n")
      .map((l) => l.split(/\s+/)[1]);
    expect(names).toEqual(["a.AppImage", "a.AppImage.sig", "b.msi"]);
    expect(sums).toContain(
      "9834876dcfb05cb167a5c24953eba58c4ac89b1adf57f28f2f9d09af107ee8f0  a.AppImage",
    );
    expect(existsSync(path.join(dir, "SHA256SUMS.tmp"))).toBe(false);
    const check = spawnSync("sha256sum", ["-c", "SHA256SUMS"], { cwd: dir });
    expect(check.status).toBe(0);
  });

  it("fails on an empty directory", () => {
    const res = run(dir);
    expect(res.status).not.toBe(0);
    expect(existsSync(path.join(dir, "SHA256SUMS"))).toBe(false);
  });

  it("fails on a missing directory", () => {
    expect(run(path.join(dir, "missing")).status).not.toBe(0);
  });
});
