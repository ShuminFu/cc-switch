import path from "node:path";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { load } from "js-yaml";
import { describe, it, expect } from "vitest";

// 执行 release.yml 中 "Validate Tauri signing key" 步骤的 run: 脚本（CCS-029 / CCS-218）：
// 只接受 `tauri signer generate` 生成的 .key 文件内容（单行 base64，解码后首行为
// "untrusted comment:"），其余输入一律以非零退出，且任何情况下都不回显密钥。
const ROOT = path.resolve(__dirname, "../..");
const STEP_NAME = "Validate Tauri signing key";

interface Step {
  name?: string;
  shell?: string;
  run?: string;
}

function validateScript(): string {
  const release = load(
    readFileSync(path.join(ROOT, ".github/workflows/release.yml"), "utf8"),
  ) as { jobs: Record<string, { steps?: Step[] }> };
  const steps = (release.jobs.release.steps ?? []).filter(
    (s) => s.name === STEP_NAME,
  );
  expect(steps).toHaveLength(1);
  expect(steps[0].shell).toBe("bash");
  expect(typeof steps[0].run).toBe("string");
  return steps[0].run!;
}

const SECRET_LINE =
  "RWRTY0IyQ0NTd2l0Y2hUZXN0T25seU5vdEFSZWFsS2V5MDEyMzQ1Njc4OWFiY2RlZg==";
const RAW_KEY = `untrusted comment: minisign encrypted secret key\n${SECRET_LINE}\n`;
const CANONICAL = Buffer.from(RAW_KEY, "utf8").toString("base64");

/** 在受控环境中运行脚本；key 为 undefined 时不设置该变量 */
function run(script: string, key: string | undefined) {
  const env: Record<string, string> = { PATH: process.env.PATH ?? "" };
  if (key !== undefined) env.TAURI_SIGNING_PRIVATE_KEY = key;
  const res = spawnSync("bash", ["-c", script], { env, encoding: "utf8" });
  return { status: res.status, output: `${res.stdout}${res.stderr}` };
}

const canRun =
  spawnSync("bash", ["--version"], { stdio: "ignore" }).status === 0 &&
  spawnSync("base64", ["--version"], { stdio: "ignore" }).status === 0;

describe.skipIf(!canRun)("Validate Tauri signing key step", () => {
  const script = validateScript();

  const expectNoLeak = (output: string, key: string) => {
    if (key.trim()) expect(output).not.toContain(key.trim());
    expect(output).not.toContain(SECRET_LINE);
    expect(output).not.toContain(CANONICAL);
  };

  it("accepts the canonical single-line base64 key without echoing it", () => {
    const res = run(script, CANONICAL);
    expect(res.status).toBe(0);
    expect(res.output).toContain("Tauri signing key format OK");
    expectNoLeak(res.output, CANONICAL);
  });

  it.each([
    ["unset", undefined],
    ["empty", ""],
  ])("rejects an %s key as missing", (_, key) => {
    const res = run(script, key);
    expect(res.status).toBe(1);
    expect(res.output).toContain("Secret 为空");
  });

  it.each([
    ["the raw two-line .key text", RAW_KEY],
    ["only the bare second line", SECRET_LINE],
    ["garbage", "not-a-key !!"],
    [
      "base64 of something else",
      Buffer.from("hello\nworld\n").toString("base64"),
    ],
  ])("rejects %s without echoing it", (_, key) => {
    const res = run(script, key);
    expect(res.status).toBe(1);
    expect(res.output).toContain("tauri signer generate");
    expectNoLeak(res.output, key);
  });

  // 守卫自检：去掉 base64/首行检查后，上面的拒绝用例必须失败
  it("would catch a validate step without the base64 check", () => {
    const start = script.indexOf("FIRST_LINE=");
    const end = script.indexOf("esac", start);
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
    const broken = script.slice(0, start) + script.slice(end + "esac".length);
    expect(run(broken, "not-a-key !!").status).toBe(0);
    expect(run(broken, RAW_KEY).status).toBe(0);
  });
});
