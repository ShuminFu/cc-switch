import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { load } from "js-yaml";
import { afterEach, beforeEach, describe, it, expect } from "vitest";

// 按 publish-release 的目录布局执行 "Generate SHA256SUMS" 步骤（CCS-033 / CCS-218）：
// 产物在 release-assets/，脚本经 sparse checkout 位于 src/scripts/；生成的 SHA256SUMS
// 必须能被 `sha256sum -c --ignore-missing` 校验，且符合 attest-build-provenance 的
// subject-checksums 格式（每行 "<64 位小写十六进制>  <不含路径的文件名>"）。
const ROOT = path.resolve(__dirname, "../..");

interface Step {
  name?: string;
  uses?: string;
  run?: string;
  with?: Record<string, unknown>;
}

const release = load(
  readFileSync(path.join(ROOT, ".github/workflows/release.yml"), "utf8"),
) as { jobs: Record<string, { steps?: Step[] }> };
const steps = release.jobs["publish-release"].steps ?? [];
const stepNamed = (name: string) => {
  const found = steps.filter((s) => s.name === name);
  expect(found).toHaveLength(1);
  return found[0];
};

const SUBJECT_LINE = /^[0-9a-f]{64} {2}[^/]+$/;
/** 不符合 subject-checksums 格式的行 */
const badLines = (sums: string) =>
  sums
    .split("\n")
    .filter((l) => l !== "")
    .filter((l) => !SUBJECT_LINE.test(l));

const ASSETS = [
  "CC-Switch-v9.9.9-Linux-x86_64.AppImage",
  "CC-Switch-v9.9.9-Linux-x86_64.AppImage.sig",
  "CC-Switch-v9.9.9-Windows.msi",
  "CC-Switch-v9.9.9-Windows.msi.sig",
  "CC-Switch-v9.9.9-macOS.tar.gz",
  "CC-Switch-v9.9.9-macOS.tar.gz.sig",
  "latest.json",
];

const canRun =
  spawnSync("bash", ["--version"], { stdio: "ignore" }).status === 0 &&
  spawnSync("sha256sum", ["--version"], { stdio: "ignore" }).status === 0;

describe("publish-release SHA256SUMS wiring", () => {
  it("checks the helper out under src/ and attests the generated file", () => {
    const checkout = stepNamed("Checkout release scripts");
    expect(checkout.uses).toMatch(/^actions\/checkout@/);
    expect(checkout.with).toMatchObject({
      path: "src",
      "sparse-checkout": "scripts",
    });
    expect(stepNamed("Generate SHA256SUMS").run).toContain(
      "src/scripts/generate-sha256sums.sh release-assets",
    );
    expect(stepNamed("Attest build provenance").with).toMatchObject({
      "subject-checksums": "release-assets/SHA256SUMS",
    });
  });
});

describe.skipIf(!canRun)(
  "Generate SHA256SUMS step in the sparse layout",
  () => {
    let dir: string;

    beforeEach(() => {
      dir = mkdtempSync(path.join(tmpdir(), "publish-release-"));
      mkdirSync(path.join(dir, "release-assets"));
      for (const name of ASSETS) {
        writeFileSync(path.join(dir, "release-assets", name), `fake ${name}\n`);
      }
      mkdirSync(path.join(dir, "src/scripts"), { recursive: true });
      copyFileSync(
        path.join(ROOT, "scripts/generate-sha256sums.sh"),
        path.join(dir, "src/scripts/generate-sha256sums.sh"),
      );
    });

    afterEach(() => {
      rmSync(dir, { recursive: true, force: true });
    });

    it("writes a SHA256SUMS that verifies and matches subject-checksums", () => {
      const script = stepNamed("Generate SHA256SUMS").run!;
      const res = spawnSync("bash", ["-c", script], {
        cwd: dir,
        encoding: "utf8",
      });
      expect(res.status, res.stderr).toBe(0);

      const sums = readFileSync(
        path.join(dir, "release-assets/SHA256SUMS"),
        "utf8",
      );
      expect(badLines(sums)).toEqual([]);
      const names = sums
        .trim()
        .split("\n")
        .map((l) => l.slice(66));
      expect(names).toEqual([...ASSETS].sort());

      const check = spawnSync(
        "sha256sum",
        ["-c", "SHA256SUMS", "--ignore-missing"],
        {
          cwd: path.join(dir, "release-assets"),
          encoding: "utf8",
        },
      );
      expect(check.status, check.stdout + check.stderr).toBe(0);
      for (const name of ASSETS) expect(check.stdout).toContain(`${name}: OK`);
    });

    // 守卫自检：带路径前缀或非标准分隔的清单必须被格式检查拒绝
    it("rejects listings with directory prefixes or the wrong separator", () => {
      const res = spawnSync("sha256sum", ["release-assets/latest.json"], {
        cwd: dir,
        encoding: "utf8",
      });
      expect(res.status).toBe(0);
      expect(badLines(res.stdout)).toHaveLength(1);
      const hash = "a".repeat(64);
      expect(badLines(`${hash} *latest.json\n`)).toHaveLength(1);
      expect(badLines(`${hash.toUpperCase()}  latest.json\n`)).toHaveLength(1);
      expect(badLines(`${hash}  latest.json\n`)).toEqual([]);
    });
  },
);
