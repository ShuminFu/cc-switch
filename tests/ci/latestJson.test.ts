import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  existsSync,
  rmSync,
  writeFileSync,
  unlinkSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";

// latest.json 生成器：任一 updater 平台缺少产物或签名都必须非零退出，且不写出清单；
// release.yml 中各平台的 Prepare Assets 步骤对缺失的 updater 产物 / .sig 直接失败（CCS-030）。
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/generate-latest-json.sh");
const RELEASE = path.join(ROOT, ".github/workflows/release.yml");

const TAG = "v9.9.9";
const UPDATER_ASSETS = [
  `CC-Switch-${TAG}-macOS.tar.gz`,
  `CC-Switch-${TAG}-Windows.msi`,
  `CC-Switch-${TAG}-Windows-arm64.msi`,
  `CC-Switch-${TAG}-Linux-x86_64.AppImage`,
  `CC-Switch-${TAG}-Linux-arm64.AppImage`,
];
const REQUIRED_KEYS = [
  "darwin-aarch64",
  "darwin-x86_64",
  "windows-x86_64",
  "windows-aarch64",
  "linux-x86_64",
  "linux-aarch64",
];

const hasTool = (cmd: string, args: string[]) =>
  spawnSync(cmd, args, { stdio: "ignore" }).status === 0;
const canRun = hasTool("bash", ["--version"]) && hasTool("jq", ["--version"]);

describe.skipIf(!canRun)("generate-latest-json.sh", () => {
  let dir: string;
  let out: string;

  const run = () =>
    spawnSync("bash", [SCRIPT, dir, "owner/repo", TAG, out], {
      encoding: "utf8",
      env: { ...process.env, PUB_DATE: "2026-01-01T00:00:00Z" },
    });

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "latest-json-"));
    for (const name of UPDATER_ASSETS) {
      writeFileSync(path.join(dir, name), "bin");
      writeFileSync(path.join(dir, `${name}.sig`), `sig:${name}\n`);
    }
    // 非 updater 产物不应影响结果
    writeFileSync(path.join(dir, `CC-Switch-${TAG}-macOS.dmg`), "dmg");
    writeFileSync(path.join(dir, `CC-Switch-${TAG}-Linux-x86_64.deb`), "deb");
    out = path.join(dir, "latest.json");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("writes all six platforms with url and signature", () => {
    const res = run();
    expect(res.status, res.stderr).toBe(0);
    const manifest = JSON.parse(readFileSync(out, "utf8"));
    expect(manifest.version).toBe("9.9.9");
    expect(manifest.pub_date).toBe("2026-01-01T00:00:00Z");
    expect(Object.keys(manifest.platforms).sort()).toEqual(
      [...REQUIRED_KEYS].sort(),
    );
    expect(manifest.platforms["windows-x86_64"]).toEqual({
      signature: `sig:CC-Switch-${TAG}-Windows.msi`,
      url: `https://github.com/owner/repo/releases/download/${TAG}/CC-Switch-${TAG}-Windows.msi`,
    });
    expect(manifest.platforms["darwin-x86_64"]).toEqual(
      manifest.platforms["darwin-aarch64"],
    );

    // 与发布流程相同的 jq -e 断言
    const check = spawnSync(
      "jq",
      [
        "-e",
        `[.platforms[${REQUIRED_KEYS.map((k) => JSON.stringify(k)).join(",")}]
          | (.url | length > 0) and (.signature | length > 0)] | all`,
        out,
      ],
      { encoding: "utf8" },
    );
    expect(check.status, check.stderr).toBe(0);
  });

  it.each(UPDATER_ASSETS)("fails when %s.sig is missing", (name) => {
    unlinkSync(path.join(dir, `${name}.sig`));
    const res = run();
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain(name);
    expect(existsSync(out)).toBe(false);
  });

  it("fails when a signature file is empty", () => {
    writeFileSync(
      path.join(dir, `CC-Switch-${TAG}-Linux-arm64.AppImage.sig`),
      "",
    );
    const res = run();
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("linux-aarch64");
    expect(existsSync(out)).toBe(false);
  });

  it.each(UPDATER_ASSETS)("fails when the %s artifact is missing", (name) => {
    unlinkSync(path.join(dir, name));
    unlinkSync(path.join(dir, `${name}.sig`));
    const res = run();
    expect(res.status).not.toBe(0);
    expect(res.stderr).toMatch(/no updater artifact found for/);
    expect(existsSync(out)).toBe(false);
  });

  it("fails when two artifacts claim the same platform", () => {
    const extra = `CC-Switch-v0.0.1-Windows.msi`;
    writeFileSync(path.join(dir, extra), "bin");
    writeFileSync(path.join(dir, `${extra}.sig`), "sig");
    const res = run();
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("windows-x86_64");
    expect(existsSync(out)).toBe(false);
  });
});

describe("release.yml updater artifacts", () => {
  const text = readFileSync(RELEASE, "utf8");

  it("generates latest.json with the validated script", () => {
    expect(text).toMatch(
      /bash scripts\/generate-latest-json\.sh dl "\$REPO" "\$TAG" latest\.json/,
    );
  });

  it("does not downgrade missing updater artifacts or signatures to warnings", () => {
    expect(text).not.toMatch(/Write-Warning\s+["'][^"']*(Signature|MSI)/i);
    expect(text).not.toMatch(/\.sig for [^"]* not found/);
    expect(text).not.toMatch(/No AppImage found[^\n]*\n\s*fi/);
  });
});
