import path from "node:path";
import { readFileSync, readdirSync } from "node:fs";
import { describe, it, expect } from "vitest";

// .node-version 是 Node 版本的唯一来源：CI / release 工作流通过 node-version-file 读取，
// package.json 的 engines.node 下限与之保持一致（CCS-026）。
const ROOT = path.resolve(__dirname, "../..");
const WORKFLOWS = path.join(ROOT, ".github/workflows");

const pinned = readFileSync(path.join(ROOT, ".node-version"), "utf8").trim();
const workflowFiles = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));

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

  it("declares engines.node matching .node-version", () => {
    const pkg = JSON.parse(
      readFileSync(path.join(ROOT, "package.json"), "utf8"),
    ) as { engines?: { node?: string } };
    expect(pkg.engines?.node).toBe(`>=${pinned}`);
  });
});
