import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect, vi } from "vitest";

// stale.yml 只能使用 actions/stale 真实存在的输入（exempt-issue-created-after 不是其输入，
// 会被忽略并产生 Unexpected input 警告）；"3+ reactions" 豁免改为由 github-script 步骤
// 给热门 issue 打 popular 标签，再通过 exempt-issue-labels 跳过（CCS-043）。
const STALE = path.resolve(__dirname, "../../.github/workflows/stale.yml");
const text = readFileSync(STALE, "utf8");
const lines = text.split(/\r?\n/);

// actions/stale v10.4.0（1e223db）action.yml 中声明的全部输入
const STALE_INPUTS = new Set(
  (
    "repo-token stale-issue-message stale-pr-message close-issue-message close-pr-message " +
    "days-before-stale days-before-issue-stale days-before-pr-stale days-before-close " +
    "days-before-issue-close days-before-pr-close stale-issue-label close-issue-label " +
    "exempt-issue-labels close-issue-reason stale-pr-label close-pr-label exempt-pr-labels " +
    "exempt-milestones exempt-issue-milestones exempt-pr-milestones exempt-all-milestones " +
    "exempt-all-issue-milestones exempt-all-pr-milestones only-labels any-of-labels " +
    "any-of-issue-labels any-of-pr-labels only-issue-labels only-pr-labels operations-per-run " +
    "remove-stale-when-updated remove-issue-stale-when-updated remove-pr-stale-when-updated " +
    "debug-only ascending sort-by delete-branch start-date exempt-assignees " +
    "exempt-issue-assignees exempt-pr-assignees exempt-all-assignees " +
    "exempt-all-issue-assignees exempt-all-pr-assignees exempt-draft-pr enable-statistics " +
    "labels-to-add-when-unstale labels-to-remove-when-stale labels-to-remove-when-unstale " +
    "ignore-updates ignore-issue-updates ignore-pr-updates include-only-assigned only-issue-types"
  ).split(" "),
);

/** 返回 `uses: <action>@` 步骤的 with: 块（键 -> 原始值），以及块内多行字符串 */
function stepWith(action: string): {
  keys: Record<string, string>;
  raw: string[];
} {
  const usesIdx = lines.findIndex((l) =>
    new RegExp(`uses:\\s*${action.replace("/", "\\/")}@`).test(l),
  );
  expect(usesIdx).toBeGreaterThan(-1);
  const withIdx = lines.findIndex(
    (l, i) => i > usesIdx && /^\s+with:\s*$/.test(l),
  );
  const indent = lines[withIdx].search(/\S/);
  const raw: string[] = [];
  for (const line of lines.slice(withIdx + 1)) {
    if (line.trim() && line.search(/\S/) <= indent) break;
    raw.push(line);
  }
  const keyIndent = raw
    .find((l) => l.trim() && !l.trim().startsWith("#"))!
    .search(/\S/);
  const keys: Record<string, string> = {};
  for (const line of raw) {
    const m = line.match(/^(\s*)([\w-]+):\s*(.*)$/);
    if (m && m[1].length === keyIndent)
      keys[m[2]] = m[3].replace(/^'(.*)'$/, "$1");
  }
  return { keys, raw };
}

/** 抽取 github-script 的 script: | 块正文（去除公共缩进） */
function labelScript(): string {
  const { raw } = stepWith("actions/github-script");
  const start = raw.findIndex((l) => /^\s*script:\s*\|\s*$/.test(l));
  const keyIndent = raw[start].search(/\S/);
  const body: string[] = [];
  for (const line of raw.slice(start + 1)) {
    if (line.trim() && line.search(/\S/) <= keyIndent) break;
    body.push(line);
  }
  const indent = Math.min(
    ...body.filter((l) => l.trim()).map((l) => l.search(/\S/)),
  );
  return body.map((l) => l.slice(indent)).join("\n");
}

function envValue(name: string): string {
  const m = text.match(new RegExp(`^\\s+${name}:\\s*'?([^'\\n]*)'?\\s*$`, "m"));
  expect(m).not.toBeNull();
  return m![1];
}

type Issue = {
  number: number;
  labels: (string | { name: string })[];
  reactions?: { total_count: number };
  pull_request?: object;
};

async function runLabelScript(issues: Issue[], debugOnly: boolean) {
  const addLabels = vi.fn().mockResolvedValue({});
  const listForRepo = vi.fn();
  const github = {
    rest: { issues: { listForRepo, addLabels } },
    paginate: vi.fn(async (fn: unknown, params: { state: string }) => {
      expect(fn).toBe(listForRepo);
      expect(params.state).toBe("open");
      return issues;
    }),
  };
  const context = { repo: { owner: "o", repo: "r" } };
  const core = { info: vi.fn() };
  const env = {
    POPULAR_LABEL: envValue("POPULAR_LABEL"),
    POPULAR_MIN_REACTIONS: envValue("POPULAR_MIN_REACTIONS"),
    DEBUG_ONLY: String(debugOnly),
  };
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const fn = new AsyncFunction(
    "github",
    "context",
    "core",
    "process",
    labelScript(),
  );
  await fn(github, context, core, { env });
  return addLabels;
}

describe("stale.yml", () => {
  it("passes only inputs that actions/stale declares", () => {
    const { keys } = stepWith("actions/stale");
    const unknown = Object.keys(keys).filter((k) => !STALE_INPUTS.has(k));
    expect(unknown).toEqual([]);
    expect(text).not.toMatch(/exempt-issue-created-after/);
  });

  it("exempts the popular label applied by the labelling step", () => {
    const { keys } = stepWith("actions/stale");
    const exempt = keys["exempt-issue-labels"].split(",").map((s) => s.trim());
    expect(exempt).toEqual(
      expect.arrayContaining([
        "security",
        "performance",
        envValue("POPULAR_LABEL"),
      ]),
    );
    expect(envValue("POPULAR_MIN_REACTIONS")).toBe("3");
    // 打标签必须发生在 stale 处理之前
    expect(text.indexOf("actions/github-script@")).toBeLessThan(
      text.indexOf("actions/stale@"),
    );
  });

  it("wires the workflow_dispatch debug-only input into both steps", () => {
    expect(text).toMatch(
      /workflow_dispatch:[\s\S]*debug-only:[\s\S]*type: boolean/,
    );
    expect(envValue("DEBUG_ONLY")).toBe("${{ inputs.debug-only == true }}");
    expect(stepWith("actions/stale").keys["debug-only"]).toBe(
      "${{ env.DEBUG_ONLY }}",
    );
  });

  it("labels open issues with 3+ reactions and skips PRs and labelled issues", async () => {
    const addLabels = await runLabelScript(
      [
        { number: 1, labels: [], reactions: { total_count: 3 } },
        { number: 2, labels: [], reactions: { total_count: 2 } },
        {
          number: 3,
          labels: [{ name: "popular" }],
          reactions: { total_count: 9 },
        },
        {
          number: 4,
          labels: [],
          reactions: { total_count: 5 },
          pull_request: {},
        },
        { number: 5, labels: ["bug"], reactions: { total_count: 12 } },
        { number: 6, labels: [] },
      ],
      false,
    );
    expect(addLabels.mock.calls.map((c) => c[0].issue_number)).toEqual([1, 5]);
    expect(addLabels).toHaveBeenCalledWith({
      owner: "o",
      repo: "r",
      issue_number: 1,
      labels: ["popular"],
    });
  });

  it("changes nothing in a debug-only run", async () => {
    const addLabels = await runLabelScript(
      [{ number: 1, labels: [], reactions: { total_count: 10 } }],
      true,
    );
    expect(addLabels).not.toHaveBeenCalled();
  });
});
