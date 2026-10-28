import path from "node:path";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";

// kpi-fix-commits.sh 统计每个发布窗口的 fix 提交数与涉及 proxy/ 的 fix 提交数，
// 作为 P0 流程规则中的 KPI 基线（CCS-034，记录在 docs/dev/ACCEPTANCE.md）。
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/kpi-fix-commits.sh");

describe("kpi-fix-commits.sh", () => {
  let dir: string;
  let n = 0;

  const git = (...args: string[]) => {
    const r = spawnSync("git", args, {
      cwd: dir,
      encoding: "utf8",
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: "t",
        GIT_AUTHOR_EMAIL: "t@example.com",
        GIT_COMMITTER_NAME: "t",
        GIT_COMMITTER_EMAIL: "t@example.com",
      },
    });
    if (r.status !== 0) throw new Error(r.stderr);
    return r.stdout.trim();
  };

  /** Commit a change to `file` with the given subject. */
  const commit = (subject: string, file = "src/a.ts") => {
    const full = path.join(dir, file);
    mkdirSync(path.dirname(full), { recursive: true });
    writeFileSync(full, `${n++}\n`);
    git("add", ".");
    git("commit", "-q", "-m", subject);
  };

  const run = (...args: string[]) =>
    spawnSync("bash", [SCRIPT, "-C", dir, ...args], { encoding: "utf8" });

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "kpi-fix-"));
    git("init", "-q", "-b", "main");
    commit("feat: initial");
    git("tag", "v1.0.0");
    commit("fix(proxy): a", "src-tauri/src/proxy/a.rs");
    commit("fix: b");
    commit("feat(proxy): c", "src-tauri/src/proxy/a.rs");
    git("tag", "v1.1.0");
    // 预发布 tag 不算一个发布窗口
    commit('Revert "feat(proxy): c"', "src-tauri/src/proxy/a.rs");
    git("tag", "v1.2.0-rc.1");
    commit("fix!: d");
    commit("chore: prefix fix: is not a fix");
    git("tag", "v1.2.0");
    commit("fix(ui): e");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("counts fix and proxy fix commits per release window", () => {
    const r = run("2");
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    const rows = r.stdout.trim().split("\n");
    expect(rows).toEqual([
      "| Release | Range | Commits | Fix commits | Fix % | Proxy fix commits |",
      "|---------|-------|--------:|------------:|------:|------------------:|",
      "| v1.1.0 | `v1.0.0..v1.1.0` | 3 | 2 | 66% | 1 |",
      "| v1.2.0 | `v1.1.0..v1.2.0` | 3 | 2 | 66% | 1 |",
      "| unreleased | `v1.2.0..HEAD` | 1 | 1 | 100% | 0 |",
    ]);
  });

  it("stops at --ref and skips the unreleased row on a tag", () => {
    const r = run("--ref", "v1.1.0", "1");
    expect(r.status).toBe(0);
    expect(r.stdout).toContain(
      "| v1.1.0 | `v1.0.0..v1.1.0` | 3 | 2 | 66% | 1 |",
    );
    expect(r.stdout).not.toContain("unreleased");
    expect(r.stdout).not.toContain("v1.2.0");
  });

  it("fails when there are not enough release tags", () => {
    const r = run("3");
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("need 4 release tags");
  });
});
