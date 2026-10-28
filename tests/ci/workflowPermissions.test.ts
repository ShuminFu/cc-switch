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

describe("workflow token permissions", () => {
  // claude.yml 只有一个 job，在 job 级别声明权限，同样不会退回仓库默认权限
  it.each(workflowFiles)("%s declares token permissions explicitly", (file) => {
    expect(read(file)).toMatch(/^\s*permissions\s*:/m);
  });

  it("ci.yml grants only contents: read", () => {
    const text = read("ci.yml");
    expect(topLevelPermissions(text)).toEqual(["contents: read"]);
    // 不允许在 job 级别重新放宽权限
    expect(text).not.toMatch(/^\s+permissions\s*:/m);
    expect(text).not.toMatch(/:\s*write\b/);
  });
});
