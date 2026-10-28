import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";

// 每个版本都以预发布形式发布，而 updater 读取 releases/latest（会跳过预发布）。
// promote-release.yml / scripts/promote-release.sh 负责把验证过的版本提升为 latest，
// 流程记录在 docs/dev/RELEASING.md（CCS-032）。
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/promote-release.sh");
const WORKFLOWS = path.join(ROOT, ".github/workflows");
const read = (rel: string) => readFileSync(path.join(ROOT, rel), "utf8");

const REPO = "owner/repo";
const TAG = "v9.9.9";

// 假 gh：按环境变量模拟 release 状态，记录调用，并让 `release edit --latest` 生效
const FAKE_GH = `#!/usr/bin/env bash
echo "$*" >> "$FAKE_DIR/calls"
case "$1 $2" in
  "release view")
    [ "$FAKE_STATE" = "missing" ] && exit 1
    echo "$FAKE_STATE" ;;
  "release download")
    dir=""
    while [ "$#" -gt 0 ]; do [ "$1" = "--dir" ] && dir="$2"; shift; done
    [ -z "$FAKE_MANIFEST" ] && exit 1
    printf '%s' "$FAKE_MANIFEST" > "$dir/latest.json" ;;
  "release edit")
    [[ " $* " == *" --latest "* ]] && echo "$3" > "$FAKE_DIR/latest" ;;
  "api repos/$FAKE_REPO/releases/latest")
    cat "$FAKE_DIR/latest" ;;
  *) echo "unexpected gh call: $*" >&2; exit 99 ;;
esac
`;

const hasTool = (cmd: string, args: string[]) =>
  spawnSync(cmd, args, { stdio: "ignore" }).status === 0;
const canRun = hasTool("bash", ["--version"]) && hasTool("jq", ["--version"]);

describe.skipIf(!canRun)("promote-release.sh", () => {
  let dir: string;

  const run = (
    tag = TAG,
    env: Partial<Record<"FAKE_STATE" | "FAKE_MANIFEST", string>> = {},
  ) =>
    spawnSync("bash", [SCRIPT, REPO, tag], {
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${dir}/bin:${process.env.PATH}`,
        FAKE_DIR: dir,
        FAKE_REPO: REPO,
        FAKE_STATE: "false true",
        FAKE_MANIFEST: JSON.stringify({ version: TAG.slice(1) }),
        ...env,
      },
    });
  const calls = () =>
    existsSync(path.join(dir, "calls"))
      ? readFileSync(path.join(dir, "calls"), "utf8")
      : "";
  const edited = () => /^release edit /m.test(calls());

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "promote-release-"));
    const bin = path.join(dir, "bin");
    spawnSync("mkdir", ["-p", bin]);
    writeFileSync(path.join(bin, "gh"), FAKE_GH);
    chmodSync(path.join(bin, "gh"), 0o755);
    writeFileSync(path.join(dir, "latest"), "v9.9.8\n");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("promotes a prerelease to latest and verifies releases/latest", () => {
    const res = run();
    expect(res.stderr).toBe("");
    expect(res.status).toBe(0);
    expect(calls()).toContain(
      `release edit ${TAG} --repo ${REPO} --prerelease=false --latest`,
    );
    expect(calls()).toContain(`api repos/${REPO}/releases/latest`);
  });

  it.each(["v1.2.3-beta.1", "1.2.3", "v1.2", "latest"])(
    "refuses non-stable tag %s without touching the release",
    (tag) => {
      const res = run(tag);
      expect(res.status).not.toBe(0);
      expect(calls()).toBe("");
    },
  );

  it("refuses a missing release", () => {
    const res = run(TAG, { FAKE_STATE: "missing" });
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("not found");
    expect(edited()).toBe(false);
  });

  it("refuses a draft release", () => {
    const res = run(TAG, { FAKE_STATE: "true true" });
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("draft");
    expect(edited()).toBe(false);
  });

  it("refuses a release without latest.json", () => {
    const res = run(TAG, { FAKE_MANIFEST: "" });
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("latest.json");
    expect(edited()).toBe(false);
  });

  it("refuses when latest.json is for another version", () => {
    const res = run(TAG, {
      FAKE_MANIFEST: JSON.stringify({ version: "9.9.8" }),
    });
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("does not match");
    expect(edited()).toBe(false);
  });

  it("fails when releases/latest still points elsewhere after the edit", () => {
    // 去掉 --latest 生效逻辑：模拟 GitHub 未切换 latest
    writeFileSync(
      path.join(dir, "bin/gh"),
      FAKE_GH.replace(`echo "$3" > "$FAKE_DIR/latest"`, "true"),
    );
    const res = run();
    expect(res.status).not.toBe(0);
    expect(res.stderr).toContain("v9.9.8");
  });
});

describe("prerelease promotion wiring", () => {
  const promote = readFileSync(
    path.join(WORKFLOWS, "promote-release.yml"),
    "utf8",
  );
  const release = readFileSync(path.join(WORKFLOWS, "release.yml"), "utf8");

  it("updater reads releases/latest, which skips prereleases", () => {
    const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
    expect(conf.plugins.updater.endpoints).toContain(
      "https://github.com/farion1231/cc-switch/releases/latest/download/latest.json",
    );
    expect(release).toMatch(/^\s+prerelease: true\s*$/m);
  });

  it("promote-release.yml is a manual workflow that runs the script", () => {
    expect(promote).toMatch(/^on:\s*\n\s+workflow_dispatch:/m);
    expect(promote).toMatch(/^\s+tag:\s*\n(?:\s+.*\n)*?\s+required: true/m);
    expect(promote).toMatch(/^ {2}promote-release:/m);
    expect(promote).toContain(
      'bash scripts/promote-release.sh "$GITHUB_REPOSITORY" "$TAG"',
    );
    // 不得直接在 run 中插值用户输入（脚本注入）
    expect(promote).not.toMatch(/run:[^\n]*\$\{\{\s*inputs\./);
    expect(promote).toContain("releases/latest/download/latest.json");
  });

  // 同组新排队的任务会取消已在等待的任务：提升不能加入 release-publish-<tag>，
  // 否则可能取消等待中的 assemble-latest-json
  it("never joins release.yml's publish concurrency group", () => {
    expect(release).toContain("group: release-publish-${{ github.ref_name }}");
    expect(promote).not.toMatch(/group: release-publish-/);
    expect(promote).toMatch(
      /group: promote-release-\$\{\{ inputs\.tag \}\}\s*\n\s+cancel-in-progress: false/,
    );
  });

  it("RELEASING.md documents promotion and is linked from CONTRIBUTING.md", () => {
    const releasing = read("docs/dev/RELEASING.md");
    expect(releasing).toContain(
      "gh release edit vX.Y.Z --repo farion1231/cc-switch --prerelease=false --latest",
    );
    expect(releasing).toContain("promote-release.yml");
    expect(read("CONTRIBUTING.md")).toContain("(./docs/dev/RELEASING.md)");
  });
});
