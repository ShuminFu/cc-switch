import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";

// release.yml 不得使用工作流级 cancel-in-progress：它会在 publish-release /
// assemble-latest-json 上传途中取消发布。构建矩阵可以被取消，发布阶段必须排队等待（CCS-031）。
const RELEASE = path.resolve(__dirname, "../../.github/workflows/release.yml");
const text = readFileSync(RELEASE, "utf8");

interface Concurrency {
  group: string | null;
  cancelInProgress: string | null;
}

/** 解析 `concurrency:` 块（key 位于 indent 列），返回 group 与 cancel-in-progress */
function parseConcurrency(lines: string[], start: number, indent: number) {
  const result: Concurrency = { group: null, cancelInProgress: null };
  for (const line of lines.slice(start + 1)) {
    if (!line.trim() || line.trim().startsWith("#")) continue;
    if (line.length - line.trimStart().length <= indent) break;
    const kv = line.trim().match(/^([\w-]+)\s*:\s*(.*?)\s*$/);
    if (!kv) continue;
    if (kv[1] === "group") result.group = kv[2];
    if (kv[1] === "cancel-in-progress") result.cancelInProgress = kv[2];
  }
  return result;
}

const lines = text.split(/\r?\n/);

/** 每个 job 的 job 级 concurrency（未声明时为 null） */
function jobConcurrency(): Record<string, Concurrency | null> {
  const start = lines.findIndex((l) => /^jobs\s*:/.test(l));
  const jobs: Record<string, Concurrency | null> = {};
  let current: string | null = null;
  lines.slice(start + 1).forEach((line, offset) => {
    const job = line.match(/^ {2}([\w-]+)\s*:/);
    if (job) {
      current = job[1];
      jobs[current] = null;
    } else if (current && /^ {4}concurrency\s*:/.test(line)) {
      jobs[current] = parseConcurrency(lines, start + 1 + offset, 4);
    }
  });
  return jobs;
}

describe("release.yml concurrency", () => {
  const jobs = jobConcurrency();

  it("has no workflow-level concurrency", () => {
    expect(lines.some((l) => /^concurrency\s*:/.test(l))).toBe(false);
  });

  it.each(["publish-release", "assemble-latest-json"])(
    "%s waits in release-publish-<ref> instead of being cancelled",
    (job) => {
      expect(jobs[job]).toEqual({
        group: "release-publish-${{ github.ref_name }}",
        cancelInProgress: "false",
      });
    },
  );

  it("build matrix stays cancellable, one group per matrix entry", () => {
    const release = jobs["release"];
    expect(release?.cancelInProgress).toBe("true");
    // 组名必须区分矩阵项，否则同一次运行的矩阵项会互相取消
    expect(release?.group).toContain("${{ matrix.os }}");
    expect(release?.group).not.toBe(jobs["publish-release"]?.group);
    // 组名按 matrix.os 区分，因此矩阵中的 os 必须唯一
    const oses = lines
      .map((l) => l.match(/^\s+- os:\s*(\S+)\s*$/)?.[1])
      .filter((os): os is string => !!os);
    expect(oses.length).toBeGreaterThan(1);
    expect(new Set(oses).size).toBe(oses.length);
  });

  it("every job declares concurrency explicitly", () => {
    const missing = Object.keys(jobs).filter((name) => jobs[name] === null);
    expect(Object.keys(jobs).length).toBeGreaterThan(0);
    expect(missing).toEqual([]);
  });
});
