import path from "node:path";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { describe, it, expect } from "vitest";

// 遮罩/全屏层守卫（CCS-200）：组件里的 `fixed inset-0` 层必须由 Radix Dialog
// 承载（焦点陷阱、Esc、aria-modal），不允许再手写 div 遮罩。
const COMPONENTS = path.resolve(__dirname, "../../src/components");

function listTsx(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listTsx(full));
    } else if (/\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

describe("custom overlays", () => {
  it("only Radix Dialog based components use fixed inset-0", () => {
    const users = listTsx(COMPONENTS).filter((file) =>
      readFileSync(file, "utf8").includes("fixed inset-0"),
    );
    // 守卫本身必须能匹配到 Radix 封装，否则说明扫描路径失效
    expect(users.map((f) => path.relative(COMPONENTS, f))).toContain(
      path.join("ui", "dialog.tsx"),
    );

    const offenders = users
      .filter(
        (file) =>
          !readFileSync(file, "utf8").includes('from "@radix-ui/react-dialog"'),
      )
      .map((file) => path.relative(COMPONENTS, file));
    expect(offenders).toEqual([]);
  });
});
