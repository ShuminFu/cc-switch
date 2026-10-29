import path from "node:path";
import { readFileSync, readdirSync } from "node:fs";
import { describe, it, expect } from "vitest";

// tauri-driver + WebdriverIO smoke（CCS-201）：ci.yml 在 debug 构建上跑，release.yml 在
// 上传前对 AppImage 跑；smoke 失败时 Linux 矩阵项不得上传产物，publish-release 随之跳过。
const ROOT = path.resolve(__dirname, "../..");
const WORKFLOWS = path.join(ROOT, ".github/workflows");
const E2E = path.join(ROOT, "e2e");

const read = (file: string) => readFileSync(file, "utf8");

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

const indexOf = (steps: Step[], name: string) => {
  const i = steps.findIndex((s) => s.name === name);
  expect(i, `step "${name}" not found`).toBeGreaterThanOrEqual(0);
  return i;
};

describe("e2e smoke wiring", () => {
  it("ci.yml runs the smoke on the debug build", () => {
    const steps = jobSteps(
      read(path.join(WORKFLOWS, "ci.yml")),
      "tauri-build-smoke",
    );
    const build = indexOf(steps, "Tauri debug build (no bundle)");
    const smoke = indexOf(steps, "E2E smoke (debug build)");
    expect(smoke).toBeGreaterThan(build);
    expect(steps[smoke].body).toMatch(/xvfb-run -a pnpm --dir e2e test/);
    expect(steps[smoke].body).not.toMatch(/continue-on-error/);
  });

  it("release.yml smokes the AppImage before any Linux asset is uploaded", () => {
    const workflow = read(path.join(WORKFLOWS, "release.yml"));
    const steps = jobSteps(workflow, "release");
    const build = indexOf(steps, "Build Tauri App (Linux)");
    const smoke = indexOf(steps, "E2E smoke (AppImage)");
    const prepare = indexOf(steps, "Prepare Linux Assets");
    const upload = indexOf(steps, "Upload release artifacts to workflow");

    expect(smoke).toBeGreaterThan(build);
    expect(smoke).toBeLessThan(prepare);
    expect(smoke).toBeLessThan(upload);

    const body = steps[smoke].body;
    expect(body).toMatch(/if: runner\.os == 'Linux'/);
    expect(body).toMatch(/\.AppImage/);
    expect(body).toMatch(/CC_SWITCH_E2E_APP=.*xvfb-run -a pnpm --dir e2e test/);
    expect(body).not.toMatch(/continue-on-error/);
    expect(body).not.toMatch(/secrets\./);
    // upload 不能用 always()/failure() 绕过前面失败的 smoke
    expect(steps[upload].body).not.toMatch(/if:.*(always|failure)\(\)/);

    // publish-release 依赖整个 release 矩阵，任一 Linux 项 smoke 失败即不发布
    const publish = workflow.split(/\n {2}publish-release:\n/)[1] ?? "";
    expect(publish).toMatch(/^ {4}needs: release$/m);
  });

  it("every test hook the spec relies on exists in src/", () => {
    const spec = readdirSync(path.join(E2E, "specs"))
      .filter((f) => f.endsWith(".e2e.mjs"))
      .map((f) => read(path.join(E2E, "specs", f)))
      .join("\n");
    const testIds = [...spec.matchAll(/data-testid="([\w-]+)"/g)].map(
      (m) => m[1],
    );
    expect(new Set(testIds)).toEqual(
      new Set([
        "first-run-notice-confirm",
        "add-provider",
        "confirm-dialog-confirm",
        "add-provider-submit",
        "provider-card",
        "provider-main-action",
        "proxy-takeover-switch",
      ]),
    );

    const srcFiles = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
        e.isDirectory()
          ? srcFiles(path.join(dir, e.name))
          : e.name.endsWith(".tsx")
            ? [path.join(dir, e.name)]
            : [],
      );
    const src = srcFiles(path.join(ROOT, "src")).map(read).join("\n");
    for (const id of testIds) {
      expect(src, `data-testid="${id}" missing from src/`).toContain(
        `data-testid="${id}"`,
      );
    }
    for (const attr of ["data-provider-name", "data-preset-name"]) {
      expect(spec).toContain(attr);
      expect(src).toContain(`${attr}={`);
    }
    // 预设名来自 claudeProviderPresets，改名会让 smoke 失效
    expect(
      read(path.join(ROOT, "src/config/claudeProviderPresets.ts")),
    ).toMatch(
      /name: "DeepSeek",\n\s+websiteUrl: "https:\/\/platform\.deepseek\.com",\n\s+settingsConfig: \{\n\s+env: \{\n\s+ANTHROPIC_BASE_URL: "https:\/\/api\.deepseek\.com\/anthropic"/,
    );
  });
});
