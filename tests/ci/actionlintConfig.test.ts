import path from "node:path";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { describe, it, expect } from "vitest";

// actionlint + shellcheck 在本地跑过 .github/workflows/*.yml 后的约束（CCS-217）：
// 重定向到 $GITHUB_OUTPUT / $GITHUB_ENV / $GITHUB_STEP_SUMMARY 必须加引号（SC2086），
// 且仓库内提交了 actionlint 配置，旧版 actionlint 也能识别 windows-11-arm runner。
const ROOT = path.resolve(__dirname, "../..");
const WORKFLOWS = path.join(ROOT, ".github/workflows");
const workflowFiles = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));

describe("actionlint/shellcheck hygiene", () => {
  it.each(workflowFiles)(
    "%s quotes redirections to GitHub runner files",
    (file) => {
      const text = readFileSync(path.join(WORKFLOWS, file), "utf8");
      const unquoted = text
        .split(/\r?\n/)
        .filter((l) => !l.trim().startsWith("#"))
        .filter((l) =>
          />>?\s*\$\{?GITHUB_(OUTPUT|ENV|STEP_SUMMARY|PATH)\b/.test(l),
        );
      expect(unquoted).toEqual([]);
    },
  );

  it("commits an actionlint config that declares windows-11-arm", () => {
    const config = path.join(ROOT, ".github/actionlint.yaml");
    expect(existsSync(config)).toBe(true);
    const text = readFileSync(config, "utf8");
    expect(text).toMatch(/^self-hosted-runner:\s*$/m);
    expect(text).toMatch(/^\s+- windows-11-arm\s*$/m);
    const usesArm = workflowFiles.some((f) =>
      readFileSync(path.join(WORKFLOWS, f), "utf8").includes("windows-11-arm"),
    );
    expect(usesArm).toBe(true);
  });
});
