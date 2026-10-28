import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, it, expect } from "vitest";
import {
  checkProxyFixture,
  parseLabels,
} from "../../scripts/check-proxy-fixture.mjs";

// 修改 src-tauri/src/proxy/** 的 PR 必须同时修改 tests/fixtures/proxy/ 下的 golden
// fixture，或带有 no-fixture-needed 标签；proxy-fixture.yml 在每个 PR 上运行该脚本（CCS-034）。
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/check-proxy-fixture.mjs");
const WORKFLOW = path.join(ROOT, ".github/workflows/proxy-fixture.yml");

const PROXY = "src-tauri/src/proxy/forwarder.rs";
const FIXTURE =
  "src-tauri/tests/fixtures/proxy/anthropic__openai_chat/new_case/meta.toml";

describe("checkProxyFixture", () => {
  it("passes when no proxy file changed", () => {
    const r = checkProxyFixture({
      files: ["src/App.tsx", "src-tauri/src/lib.rs"],
      labels: [],
    });
    expect(r.ok).toBe(true);
    expect(r.proxyFiles).toEqual([]);
  });

  it("fails a proxy change without a fixture or the label", () => {
    const r = checkProxyFixture({
      files: [PROXY, "src-tauri/src/lib.rs"],
      labels: ["backend", "proxy"],
    });
    expect(r.ok).toBe(false);
    expect(r.proxyFiles).toEqual([PROXY]);
  });

  it("passes a proxy change that comes with a fixture", () => {
    const r = checkProxyFixture({ files: [PROXY, FIXTURE], labels: [] });
    expect(r.ok).toBe(true);
    expect(r.fixtureFiles).toEqual([FIXTURE]);
  });

  it("passes a proxy change labelled no-fixture-needed", () => {
    const r = checkProxyFixture({
      files: [PROXY],
      labels: ["no-fixture-needed"],
    });
    expect(r.ok).toBe(true);
    expect(r.skipped).toBe(true);
  });

  it("does not count other fixtures or look-alike paths", () => {
    expect(
      checkProxyFixture({
        files: [PROXY, "src-tauri/tests/fixtures/db/v15.sql"],
        labels: [],
      }).ok,
    ).toBe(false);
    expect(
      checkProxyFixture({
        files: ["src-tauri/src/proxy_utils.rs", "src/components/proxy/X.tsx"],
        labels: [],
      }).ok,
    ).toBe(true);
  });

  it("parses PR_LABELS as a JSON array", () => {
    expect(parseLabels(undefined)).toEqual([]);
    expect(parseLabels("")).toEqual([]);
    expect(parseLabels('["proxy","no-fixture-needed"]')).toEqual([
      "proxy",
      "no-fixture-needed",
    ]);
    expect(() => parseLabels('"proxy"')).toThrow(/JSON array/);
  });
});

describe("check-proxy-fixture.mjs on a test PR", () => {
  let dir: string;
  let base: string;

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

  const write = (rel: string, text: string) => {
    mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
    writeFileSync(path.join(dir, rel), text);
  };

  const run = (labels?: string[]) =>
    spawnSync(process.execPath, [SCRIPT], {
      cwd: dir,
      encoding: "utf8",
      env: {
        ...process.env,
        BASE_SHA: base,
        HEAD_SHA: "HEAD",
        PR_LABELS: labels ? JSON.stringify(labels) : "",
      },
    });

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "proxy-fixture-"));
    git("init", "-q", "-b", "main");
    write(PROXY, "fn a() {}\n");
    git("add", ".");
    git("commit", "-q", "-m", "base");
    base = git("rev-parse", "HEAD");
    git("checkout", "-q", "-b", "pr");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("fails a PR touching proxy/ without a fixture or label", () => {
    write(PROXY, "fn a() { /* fix */ }\n");
    git("commit", "-q", "-am", "fix(proxy): x");
    const r = run(["backend"]);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain(PROXY);
    expect(r.stderr).toContain("no-fixture-needed");
  });

  it("passes the same PR once it adds a fixture", () => {
    write(PROXY, "fn a() { /* fix */ }\n");
    write(FIXTURE, 'intent = "x"\n');
    git("add", ".");
    git("commit", "-q", "-m", "fix(proxy): x");
    const r = run();
    expect(r.status).toBe(0);
    expect(r.stdout).toContain("fixture");
  });

  it("passes the same PR with the no-fixture-needed label", () => {
    write(PROXY, "fn a() { /* refactor */ }\n");
    git("commit", "-q", "-am", "refactor(proxy): x");
    expect(run(["no-fixture-needed"]).status).toBe(0);
  });

  it("ignores proxy commits that landed on main after the branch point", () => {
    git("checkout", "-q", "main");
    write(PROXY, "fn b() {}\n");
    git("commit", "-q", "-am", "main moved");
    base = git("rev-parse", "HEAD");
    git("checkout", "-q", "pr");
    write("README.md", "docs\n");
    git("add", ".");
    git("commit", "-q", "-m", "docs: x");
    expect(run().status).toBe(0);
  });
});

describe("proxy-fixture.yml", () => {
  const text = readFileSync(WORKFLOW, "utf8");

  it("re-runs when labels change", () => {
    const types = text.match(/^\s+types:\s*\[([^\]]*)\]/m)?.[1] ?? "";
    expect(types.split(",").map((t) => t.trim())).toEqual(
      expect.arrayContaining([
        "opened",
        "synchronize",
        "reopened",
        "labeled",
        "unlabeled",
      ]),
    );
  });

  it("passes the PR shas and labels to the script via env", () => {
    expect(text).toMatch(/^\s+fetch-depth:\s*0\s*$/m);
    expect(text).toContain(
      "BASE_SHA: ${{ github.event.pull_request.base.sha }}",
    );
    expect(text).toContain(
      "HEAD_SHA: ${{ github.event.pull_request.head.sha }}",
    );
    expect(text).toContain(
      "PR_LABELS: ${{ toJSON(github.event.pull_request.labels.*.name) }}",
    );
    expect(text).toMatch(
      /^\s+run: node scripts\/check-proxy-fixture\.mjs\s*$/m,
    );
  });

  it("does not list no-fixture-needed in labeler.yml", () => {
    // sync-labels 会移除 labeler.yml 中配置但条件不满足的标签，手动标签不能写进去
    const labeler = readFileSync(
      path.join(ROOT, ".github/labeler.yml"),
      "utf8",
    );
    expect(labeler).not.toMatch(/^no-fixture-needed\s*:/m);
  });
});
