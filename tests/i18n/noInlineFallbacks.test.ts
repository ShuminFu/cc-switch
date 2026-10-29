import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { describe, it, expect } from "vitest";

// CCS-199：翻译文案只允许来自语言包。t() / i18n.t() 不得再携带内联兜底文案
// （{ defaultValue } 选项或第二个参数直接传字符串），否则缺失的 key 会静默显示硬编码中文，
// 而不是被 typecheck（i18next.d.ts 的 CustomTypeOptions）和 localeParity 测试拦下。
// 组件/预设数据里与 i18n 无关的 defaultValue（表单默认值、Tabs/Accordion 属性等）不受影响。
const SRC_DIR = path.resolve(__dirname, "../../src");

const T_CALLEE = /^(?:t|i18n\.t|i18next\.t)$/;

const listSourceFiles = (dir: string): string[] =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx)$/.test(entry.name) && !/\.d\.ts$/.test(entry.name)
      ? [full]
      : [];
  });

const isStringish = (node: ts.Node) =>
  ts.isStringLiteral(node) ||
  ts.isNoSubstitutionTemplateLiteral(node) ||
  ts.isTemplateExpression(node);

/** 返回一个文件中所有带内联兜底文案的 t() 调用位置 */
const findInlineFallbacks = (fileName: string, source: string) => {
  const sf = ts.createSourceFile(
    fileName,
    source,
    ts.ScriptTarget.Latest,
    true,
    fileName.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const hits: string[] = [];
  const visit = (node: ts.Node) => {
    if (
      ts.isCallExpression(node) &&
      T_CALLEE.test(node.expression.getText(sf)) &&
      node.arguments.length >= 2
    ) {
      const [, second] = node.arguments;
      const hasDefaultValueOption =
        ts.isObjectLiteralExpression(second) &&
        second.properties.some(
          (prop) => prop.name?.getText(sf) === "defaultValue",
        );
      if (hasDefaultValueOption || isStringish(second)) {
        const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
        hits.push(`${fileName}:${line + 1} ${node.getText(sf).slice(0, 80)}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return hits;
};

describe("t() calls carry no inline fallback text", () => {
  it("detects both fallback forms (guards the scanner itself)", () => {
    const sample = [
      't("a.b", { defaultValue: "x" });',
      't("a.b", "x");',
      'i18n.t("a.b", { count: 1, defaultValue: `n` });',
      't("a.b", { count: 1 });',
      '<Tabs defaultValue="logs" />;',
      "const v = { defaultValue: 1 };",
    ].join("\n");
    expect(findInlineFallbacks("sample.tsx", sample)).toHaveLength(3);
  });

  it("src/ has none", () => {
    const hits = listSourceFiles(SRC_DIR).flatMap((file) =>
      findInlineFallbacks(
        path.relative(SRC_DIR, file).split(path.sep).join("/"),
        fs.readFileSync(file, "utf8"),
      ),
    );
    expect(hits).toEqual([]);
  });
});
