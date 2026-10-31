import path from "node:path";
import { readFileSync } from "node:fs";
import { load } from "js-yaml";
import { describe, it, expect } from "vitest";

// 发布门禁（CCS-023 / CCS-218）：release.yml 先以 workflow_call 跑完整的 ci.yml，
// release → publish-release → assemble-latest-json 逐级依赖，任何一环都不能被
// always()/!cancelled()/failure()、按分支或事件跳过的 CI job 或 continue-on-error 绕过。
const ROOT = path.resolve(__dirname, "../..");
const loadWorkflow = (rel: string) =>
  load(readFileSync(path.join(ROOT, rel), "utf8")) as Workflow;

interface Step {
  name?: string;
  if?: unknown;
  "continue-on-error"?: unknown;
}

interface Job {
  if?: unknown;
  needs?: string | string[];
  uses?: string;
  "continue-on-error"?: unknown;
  steps?: Step[];
}

interface Workflow {
  on: Record<string, unknown>;
  jobs: Record<string, Job>;
}

/** 必须全绿才能发布的 CI job；只有覆盖率 job 允许 continue-on-error */
const GATING_CI_JOBS = ["frontend", "backend", "msrv", "tauri-build-smoke"];
const SOFT_CI_JOBS = ["coverage-frontend", "coverage-rust"];
/** 发布链路上的 job：任一 job 级 if: 都不能让失败的上游放行 */
const RELEASE_CHAIN = [
  "ci",
  "check-versions",
  "release",
  "publish-release",
  "assemble-latest-json",
];

const BYPASS_CONDITION = /\balways\s*\(|!\s*cancelled\s*\(|\bfailure\s*\(/;
// 依赖 ref / 事件的条件在 tag push（release.yml 经 workflow_call 调用）时可能把 job 跳过，
// 而被跳过的 job 在调用方看来不算失败
const REF_OR_EVENT_CONDITION =
  /github\.(ref|ref_name|ref_type|event_name|head_ref|base_ref)\b|refs\/(heads|tags)\//;

const needsOf = (job: Job): string[] =>
  job.needs === undefined
    ? []
    : Array.isArray(job.needs)
      ? job.needs
      : [job.needs];

const isTruthyFlag = (v: unknown) => v !== undefined && v !== false;

/** 发布链路中 job 级 if: 使用 always()/!cancelled()/failure() 的 job */
function bypassingReleaseJobs(release: Workflow): string[] {
  return RELEASE_CHAIN.filter((name) => {
    const cond = release.jobs[name]?.if;
    return cond !== undefined && BYPASS_CONDITION.test(String(cond));
  });
}

/** ci.yml 中 job 级 if: 依赖 ref / 事件（可能在 tag 或 workflow_call 下被跳过）的 job */
function refGatedCiJobs(ci: Workflow): string[] {
  return Object.entries(ci.jobs)
    .filter(
      ([name, job]) =>
        job.if !== undefined &&
        (GATING_CI_JOBS.includes(name) ||
          REF_OR_EVENT_CONDITION.test(String(job.if))),
    )
    .map(([name]) => name);
}

/** 必需的 CI job 或其步骤上出现 continue-on-error 的位置 */
function softFailingGateJobs(ci: Workflow): string[] {
  const bad: string[] = [];
  for (const name of GATING_CI_JOBS) {
    const job = ci.jobs[name];
    if (!job) {
      bad.push(`${name} (missing)`);
      continue;
    }
    if (isTruthyFlag(job["continue-on-error"])) bad.push(name);
    for (const step of job.steps ?? []) {
      if (isTruthyFlag(step["continue-on-error"])) {
        bad.push(`${name} › ${step.name ?? "<unnamed step>"}`);
      }
    }
  }
  return bad;
}

const clone = <T>(v: T): T => structuredClone(v);

describe("release gate (parsed workflows)", () => {
  const ci = loadWorkflow(".github/workflows/ci.yml");
  const release = loadWorkflow(".github/workflows/release.yml");

  it("ci.yml can be called by release.yml", () => {
    expect(Object.keys(ci.on)).toContain("workflow_call");
  });

  it("release.yml runs ci.yml as its ci job", () => {
    expect(release.jobs.ci?.uses).toBe("./.github/workflows/ci.yml");
  });

  it("the release chain waits on each previous stage", () => {
    expect(needsOf(release.jobs.release)).toEqual(
      expect.arrayContaining(["ci", "check-versions"]),
    );
    expect(needsOf(release.jobs["publish-release"])).toContain("release");
    expect(needsOf(release.jobs["assemble-latest-json"])).toContain(
      "publish-release",
    );
  });

  it("no release-chain job condition lets a failed upstream through", () => {
    expect(bypassingReleaseJobs(release)).toEqual([]);
  });

  it("no ci.yml job is skipped by ref or event (tag push / workflow_call)", () => {
    expect(refGatedCiJobs(ci)).toEqual([]);
  });

  it("gating ci.yml jobs never continue on error; only coverage may", () => {
    expect(softFailingGateJobs(ci)).toEqual([]);
    const soft = Object.entries(ci.jobs)
      .filter(([, job]) => isTruthyFlag(job["continue-on-error"]))
      .map(([name]) => name);
    for (const name of soft) expect(SOFT_CI_JOBS).toContain(name);
  });
});

// 守卫自检：在解析后的副本上破坏被守护的属性，确认对应检查会报错
describe("release gate guards catch broken copies", () => {
  const ci = loadWorkflow(".github/workflows/ci.yml");
  const release = loadWorkflow(".github/workflows/release.yml");

  it.each([
    ["release", "always()"],
    ["publish-release", "always()"],
    ["publish-release", "${{ !cancelled() }}"],
    ["assemble-latest-json", "success() || failure()"],
  ])("flags %s with if: %s", (job, cond) => {
    const broken = clone(release);
    broken.jobs[job].if = cond;
    expect(bypassingReleaseJobs(broken)).toEqual([job]);
  });

  it.each([
    ["frontend", "github.ref == 'refs/heads/main'"],
    ["backend", "github.event_name == 'pull_request'"],
    ["coverage-rust", "startsWith(github.ref, 'refs/tags/')"],
  ])("flags ci.yml %s with if: %s", (job, cond) => {
    const broken = clone(ci);
    broken.jobs[job].if = cond;
    expect(refGatedCiJobs(broken)).toEqual([job]);
  });

  it.each(GATING_CI_JOBS)("flags continue-on-error on %s", (job) => {
    const broken = clone(ci);
    broken.jobs[job]["continue-on-error"] = true;
    expect(softFailingGateJobs(broken)).toEqual([job]);
  });

  it("flags continue-on-error on a gating job's step", () => {
    const broken = clone(ci);
    const steps = broken.jobs.backend.steps!;
    steps[steps.length - 1]["continue-on-error"] = true;
    expect(softFailingGateJobs(broken)).toHaveLength(1);
  });
});
