import fs from "node:fs";
import path from "node:path";
import i18next, { type TFunction } from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import {
  createProviderSchema,
  parseJsonError,
  providerSchema,
} from "@/lib/schemas/provider";
import {
  updateCommonConfigSnippet,
  validateJsonConfig,
} from "@/utils/providerConfigUtils";
import { tomlToMcpServer } from "@/utils/tomlUtils";

// CCS-193：校验文案必须走语言包，英文界面下不能出现硬编码中文。
const CJK = /[一-鿿]/;

const makeT = async (lng: "en" | "zh"): Promise<TFunction> => {
  const instance = i18next.createInstance();
  await instance.init({
    lng,
    resources: { en: { translation: en }, zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
  return instance.t;
};

const issueMessages = (
  result: ReturnType<ReturnType<typeof createProviderSchema>["safeParse"]>,
) => (result.success ? [] : result.error.issues.map((issue) => issue.message));

describe("createProviderSchema(t)", () => {
  it("reports malformed settingsConfig without CJK in English", async () => {
    const t = await makeT("en");
    const result = createProviderSchema(t).safeParse({
      name: "x",
      settingsConfig: "{bad",
    });

    const messages = issueMessages(result);
    expect(messages).toHaveLength(1);
    expect(messages[0]).toMatch(/^JSON format error/);
    expect(messages.join("\n")).not.toMatch(CJK);
  });

  it("localizes the required and URL messages", async () => {
    const t = await makeT("en");
    const result = createProviderSchema(t).safeParse({
      name: "x",
      websiteUrl: "not a url",
      settingsConfig: "",
    });

    const messages = issueMessages(result);
    expect(messages).toContain("Please enter a valid URL");
    expect(messages).toContain("Please fill in the config content");
    expect(messages.join("\n")).not.toMatch(CJK);
  });

  it("still yields Chinese messages for the zh locale", async () => {
    const t = await makeT("zh");
    const result = createProviderSchema(t).safeParse({
      name: "x",
      settingsConfig: "",
    });

    expect(issueMessages(result)).toContain("请填写配置内容");
  });
});

describe("parseJsonError", () => {
  it("formats position, line/column and generic engine messages", async () => {
    const t = await makeT("en");

    expect(
      parseJsonError(
        new SyntaxError("Unexpected token b in JSON at position 1"),
        t,
      ),
    ).toBe("JSON format error: Unexpected token b (position 1)");
    expect(
      parseJsonError(
        new SyntaxError(
          "JSON.parse: unexpected character at line 2 column 3 of the JSON data",
        ),
        t,
      ),
    ).toBe("JSON format error: line 2, column 3");
    expect(parseJsonError(new SyntaxError("Bad thing"), t)).toBe(
      "JSON format error: Bad thing",
    );
    expect(parseJsonError(new Error("x"), t)).toBe("Config JSON format error");
  });
});

describe("validateJsonConfig / updateCommonConfigSnippet", () => {
  it("returns localized messages naming the field", async () => {
    const t = await makeT("en");

    expect(validateJsonConfig("[]", t, "commonConfigSnippet")).toBe(
      "Common config snippet must be a JSON object",
    );
    expect(validateJsonConfig("{bad", t, "extractedConfig")).toBe(
      "Extracted config has a JSON format error, please check the syntax",
    );
    expect(validateJsonConfig("{}", t)).toBe("");

    expect(updateCommonConfigSnippet("{bad", "{}", true, t).error).toBe(
      "Failed to parse the config JSON; cannot apply the common config",
    );
    const snippetError = updateCommonConfigSnippet("{}", "[1]", true, t).error;
    expect(snippetError).toBe("Common config snippet must be a JSON object");
  });
});

describe("static schemas follow the global language", () => {
  afterEach(async () => {
    await i18next.changeLanguage("zh");
  });

  it("providerSchema and tomlToMcpServer use the current i18next language", async () => {
    i18next.addResourceBundle("en", "translation", en, true, true);
    await i18next.changeLanguage("en");

    const result = providerSchema.safeParse({
      name: "x",
      settingsConfig: "{bad",
    });
    const messages = issueMessages(result);
    expect(messages[0]).toMatch(/^JSON format error/);
    expect(messages.join("\n")).not.toMatch(CJK);

    expect(() => tomlToMcpServer("")).toThrow("TOML content cannot be empty");
    expect(() => tomlToMcpServer('type = "stdio"')).toThrow(
      "An stdio MCP server must include the command field",
    );
  });
});

// CCS-253：被扫描目录的 .ts 源码里，CJK 只允许出现在注释中（不能是字符串字面量或
// 文案匹配器）。
describe("CJK-free source outside comments", () => {
  const ROOT = path.resolve(__dirname, "..", "..");
  const SCANNED = [
    "src/lib",
    "src/utils",
    "src/components/providers/forms/hooks",
  ];
  const tsFiles = (dir: string): string[] =>
    fs
      .readdirSync(dir, { withFileTypes: true })
      .flatMap((e) =>
        e.isDirectory()
          ? tsFiles(path.join(dir, e.name))
          : e.name.endsWith(".ts")
            ? [path.join(dir, e.name)]
            : [],
      );
  const stripComments = (source: string) =>
    source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");

  it("has no CJK in code in src/lib, src/utils and provider form hooks", () => {
    const offenders = SCANNED.flatMap((dir) =>
      tsFiles(path.join(ROOT, dir)),
    ).flatMap((file) =>
      stripComments(fs.readFileSync(file, "utf8"))
        .split("\n")
        .filter((line) => CJK.test(line))
        .map((line) => `${path.relative(ROOT, file)}: ${line.trim()}`),
    );
    expect(offenders).toEqual([]);
  });
});
