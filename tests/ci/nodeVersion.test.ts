import path from "node:path";
import { readFileSync, readdirSync } from "node:fs";
import { load } from "js-yaml";
import { describe, it, expect } from "vitest";

// .node-version 是 Node 版本的唯一来源：CI / release 工作流通过 node-version-file 读取，
// package.json 的 engines.node 下限与之保持一致（CCS-026）。
const ROOT = path.resolve(__dirname, "../..");
const WORKFLOWS = path.join(ROOT, ".github/workflows");

const pinned = readFileSync(path.join(ROOT, ".node-version"), "utf8").trim();
const workflowFiles = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));

interface Step {
  uses?: string;
  run?: string;
  shell?: string;
}

// 检查步骤必须用 bash（release 矩阵含 Windows，默认 shell 是 pwsh），
// 打印版本并与 .node-version 比较。
const isNodeVersionCheck = (step: Step | undefined): boolean =>
  step?.shell === "bash" &&
  typeof step.run === "string" &&
  /^node --version$/m.test(step.run) &&
  /test "\$\(node --version\)" = "v\$\(tr -d '\[:space:\]' < \.node-version\)"/.test(
    step.run,
  );

describe("node version pinning", () => {
  it("pins an exact version in .node-version", () => {
    expect(pinned).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it.each(workflowFiles)(
    "%s reads the Node version from .node-version",
    (file) => {
      const text = readFileSync(path.join(WORKFLOWS, file), "utf8");
      // 不允许硬编码 node-version: "20" 之类的值
      expect(text).not.toMatch(/^\s*node-version\s*:/m);
      const setupNodeSteps = text.match(/uses:\s*actions\/setup-node@/g) ?? [];
      const versionFileRefs =
        text.match(/^\s*node-version-file:\s*\.node-version\s*$/gm) ?? [];
      expect(versionFileRefs.length).toBe(setupNodeSteps.length);
    },
  );

  // setup-node 解析出的版本若与 .node-version 漂移会静默通过，因此每个
  // setup-node 步骤后必须紧跟一个断言 node --version 的检查步骤（CCS-219）。
  it.each(workflowFiles)(
    "%s asserts the resolved Node version right after setup-node",
    (file) => {
      const doc = load(readFileSync(path.join(WORKFLOWS, file), "utf8")) as {
        jobs?: Record<string, { steps?: Step[] }>;
      };
      for (const [job, { steps = [] }] of Object.entries(doc.jobs ?? {})) {
        steps.forEach((step, i) => {
          if (!step.uses?.startsWith("actions/setup-node@")) return;
          const next = steps[i + 1];
          expect(next, `${job} step ${i + 1}`).toBeDefined();
          expect(isNodeVersionCheck(next), `${job} step ${i + 1}`).toBe(true);
        });
      }
    },
  );

  it("declares engines.node matching .node-version", () => {
    const pkg = JSON.parse(
      readFileSync(path.join(ROOT, "package.json"), "utf8"),
    ) as { engines?: { node?: string } };
    expect(pkg.engines?.node).toBe(`>=${pinned}`);
  });
});
