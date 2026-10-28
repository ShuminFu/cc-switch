import path from "node:path";
import { readFileSync, readdirSync } from "node:fs";
import { describe, it, expect } from "vitest";

// 每个工作流都必须显式声明 permissions（顶层或 job 级），避免 GITHUB_TOKEN 退回仓库默认权限
// （可能是 write-all）；ci.yml 只需读取仓库内容（CCS-027）。
const WORKFLOWS = path.resolve(__dirname, "../../.github/workflows");
const workflowFiles = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));

const read = (file: string) => readFileSync(path.join(WORKFLOWS, file), "utf8");

/** 返回顶层 permissions 块（不含键本身）的非空、非注释行，缺失时返回 null */
function topLevelPermissions(text: string): string[] | null {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => /^permissions\s*:/.test(l));
  if (start === -1) return null;
  const inline = lines[start].replace(/^permissions\s*:/, "").trim();
  if (inline && !inline.startsWith("#")) return [inline];
  const body: string[] = [];
  for (const line of lines.slice(start + 1)) {
    if (/^\S/.test(line)) break;
    const t = line.trim();
    if (t && !t.startsWith("#")) body.push(t);
  }
  return body;
}

/** 返回 jobs 下每个 job 的名称及其是否在 job 级别声明了 permissions */
function jobPermissions(text: string): Record<string, boolean> {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => /^jobs\s*:/.test(l));
  const jobs: Record<string, boolean> = {};
  if (start === -1) return jobs;
  let current: string | null = null;
  for (const line of lines.slice(start + 1)) {
    if (/^\S/.test(line) && !line.startsWith("#")) break;
    const job = line.match(/^ {2}([\w-]+)\s*:/);
    if (job) {
      current = job[1];
      jobs[current] = false;
    } else if (current && /^ {4}permissions\s*:/.test(line)) {
      jobs[current] = true;
    }
  }
  return jobs;
}

describe("workflow token permissions", () => {
  // 没有顶层 permissions 时（如 claude.yml），每个 job 都必须在 job 级别声明，
  // 否则未声明的 job 仍会退回仓库默认权限
  it.each(workflowFiles)("%s declares token permissions explicitly", (file) => {
    const text = read(file);
    if (topLevelPermissions(text) !== null) return;
    const jobs = jobPermissions(text);
    expect(Object.keys(jobs).length).toBeGreaterThan(0);
    const missing = Object.keys(jobs).filter((name) => !jobs[name]);
    expect(missing).toEqual([]);
  });

  it("ci.yml grants only contents: read", () => {
    const text = read("ci.yml");
    expect(topLevelPermissions(text)).toEqual(["contents: read"]);
    // 不允许在 job 级别重新放宽权限
    expect(text).not.toMatch(/^\s+permissions\s*:/m);
    expect(text).not.toMatch(/:\s*write\b/);
  });
});
