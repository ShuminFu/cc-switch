import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";

// Track F 退出标准（CCS-250）：App.tsx 不超过 300 行，
// 新的外壳逻辑应放到 src/components/app-shell/ 下的组件或 Hook 中。
const ROOT = path.resolve(__dirname, "../..");
const APP_LINE_BUDGET = 300;

describe("app shell size", () => {
  it(`keeps src/App.tsx within ${APP_LINE_BUDGET} lines`, () => {
    const source = readFileSync(path.join(ROOT, "src/App.tsx"), "utf8");
    const lines = source.replace(/\n$/, "").split("\n").length;
    expect(lines).toBeLessThanOrEqual(APP_LINE_BUDGET);
  });
});
