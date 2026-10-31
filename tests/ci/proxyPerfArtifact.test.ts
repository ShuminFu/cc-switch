import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";

// proxy_e2e 性能基线（CCS-014）写到 CARGO_TARGET_TMPDIR/proxy-e2e-perf.json；
// backend 矩阵在 "Run tests" 之后按 OS 上传该文件，测试失败时也保留（CCS-213）。
const ROOT = path.resolve(__dirname, "../..");
const read = (rel: string) => readFileSync(path.join(ROOT, rel), "utf8");
const PERF_PATH = "src-tauri/target/tmp/proxy-e2e-perf.json";

interface Step {
  name: string;
  body: string;
}

/** 取出某个 job 的所有 step（按出现顺序），body 为该 step 的完整文本 */
function jobSteps(workflow: string, job: string): Step[] {
  const lines = workflow.split(/\r?\n/);
  const start = lines.findIndex((l) => l === `  ${job}:`);
  expect(start, `job ${job} not found`).toBeGreaterThanOrEqual(0);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^ {2}[\w-]+:/.test(lines[i])) {
      end = i;
      break;
    }
  }
  const steps: Step[] = [];
  for (const line of lines.slice(start + 1, end)) {
    const m = line.match(/^ {6}- name:\s*(.+?)\s*$/);
    if (m) steps.push({ name: m[1], body: line });
    else if (steps.length) steps[steps.length - 1].body += `\n${line}`;
  }
  return steps;
}

describe("proxy e2e perf artifact", () => {
  it("proxy_e2e still writes the perf JSON under CARGO_TARGET_TMPDIR", () => {
    const src = read("src-tauri/tests/proxy_e2e.rs");
    expect(src).toMatch(
      /env!\("CARGO_TARGET_TMPDIR"\)\)\s*\.join\("proxy-e2e-perf\.json"\)/,
    );
  });

  it("the backend matrix uploads it per OS after Run tests, even on failure", () => {
    const steps = jobSteps(read(".github/workflows/ci.yml"), "backend");
    const tests = steps.findIndex((s) => s.name === "Run tests");
    const upload = steps.findIndex(
      (s) => s.name === "Upload proxy e2e perf artifact",
    );
    expect(tests).toBeGreaterThanOrEqual(0);
    expect(upload).toBeGreaterThan(tests);

    const body = steps[upload].body;
    expect(body).toMatch(/^ {8}if: always\(\)$/m);
    expect(body).toMatch(/uses: actions\/upload-artifact@[0-9a-f]{40} # v/);
    expect(body).toContain("name: proxy-e2e-perf-${{ matrix.os }}");
    expect(body).toContain(`path: ${PERF_PATH}`);
    expect(body).toContain("if-no-files-found: warn");
  });
});
