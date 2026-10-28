import path from "node:path";
import { readFileSync, readdirSync } from "node:fs";
import { describe, it, expect } from "vitest";

// 发布密钥只能通过 step 的 env:/with: 传入，不得内联进 run: 脚本；Tauri 签名私钥及其密码
// 只出现在三个 Build Tauri App 步骤（外加格式校验步骤）的 env 中，且不写入 $GITHUB_ENV（CCS-029）。
const WORKFLOWS = path.resolve(__dirname, "../../.github/workflows");
const workflowFiles = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));

const read = (file: string) => readFileSync(path.join(WORKFLOWS, file), "utf8");
const indentOf = (line: string) => line.length - line.trimStart().length;

interface SecretRef {
  line: number;
  text: string;
  /** 最近的上级 key（如 env / with / run） */
  parent: string | null;
  /** 所在 step 的 name，不在 step 内时为 null */
  step: string | null;
}

/** 找出所有引用 secrets.* 的行，并记录其上级 key 与所在 step */
function secretRefs(text: string): SecretRef[] {
  const lines = text.split(/\r?\n/);
  const refs: SecretRef[] = [];
  let step: string | null = null;
  lines.forEach((line, i) => {
    const stepStart = line.match(/^\s*- name:\s*(.+?)\s*$/);
    if (stepStart) step = stepStart[1];
    else if (/^ {2}[\w-]+\s*:/.test(line)) step = null; // 进入新的 job
    if (!/\bsecrets\./.test(line) || line.trim().startsWith("#")) return;
    let parent: string | null = null;
    const indent = indentOf(line);
    for (let j = i - 1; j >= 0; j--) {
      const prev = lines[j];
      if (!prev.trim() || prev.trim().startsWith("#")) continue;
      if (indentOf(prev) < indent) {
        parent = prev.trim().replace(/^- /, "").split(":")[0];
        break;
      }
    }
    refs.push({ line: i + 1, text: line.trim(), parent, step });
  });
  return refs;
}

describe("release secrets handling", () => {
  it.each(workflowFiles)(
    "%s references secrets only as env:/with: mappings",
    (file) => {
      const bad = secretRefs(read(file)).filter(
        (r) =>
          !(r.parent === "env" || r.parent === "with") ||
          !/^[\w-]+:\s*\$\{\{\s*secrets\.\w+\s*\}\}$/.test(r.text),
      );
      expect(bad).toEqual([]);
    },
  );

  it.each(workflowFiles)(
    "%s never writes Tauri signing material to GITHUB_ENV",
    (file) => {
      const offending = read(file)
        .split(/\r?\n/)
        .filter((l) => /GITHUB_ENV/.test(l) && /TAURI_SIGNING/.test(l));
      expect(offending).toEqual([]);
    },
  );

  it("release.yml scopes the Tauri signing key to the build steps", () => {
    const refs = secretRefs(read("release.yml")).filter((r) =>
      /secrets\.TAURI_SIGNING_/.test(r.text),
    );
    const steps = (secret: string) =>
      refs
        .filter((r) => r.text.includes(`secrets.${secret} `))
        .map((r) => r.step);

    const buildSteps = [
      "Build Tauri App (macOS)",
      "Build Tauri App (Windows)",
      "Build Tauri App (Linux)",
    ];
    expect(steps("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")).toEqual(buildSteps);
    expect(steps("TAURI_SIGNING_PRIVATE_KEY")).toEqual([
      "Validate Tauri signing key",
      ...buildSteps,
    ]);
    // 环境变量名必须与 secret 同名，避免在其他步骤中被改名转存
    for (const r of refs) {
      const [key] = r.text.split(":");
      expect(r.text).toContain(`secrets.${key} `);
    }
  });
});
