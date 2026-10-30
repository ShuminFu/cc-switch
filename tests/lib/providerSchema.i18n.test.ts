import i18n from "i18next";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import { parseJsonError, providerSchema } from "@/lib/schemas/provider";
import {
  updateCommonConfigSnippet,
  validateJsonConfig,
} from "@/utils/providerConfigUtils";

const CJK = /[぀-ヿ一-鿿]/;

const issueMessages = (input: Record<string, unknown>): string[] => {
  const result = providerSchema.safeParse({
    name: "p",
    settingsConfig: "{}",
    ...input,
  });
  return result.success ? [] : result.error.issues.map((i) => i.message);
};

describe("provider validation messages follow the UI language", () => {
  let previous: string;

  beforeAll(() => {
    previous = i18n.language;
    i18n.addResourceBundle("en", "translation", en, true, true);
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    i18n.addResourceBundle("ja", "translation", ja, true, true);
  });

  afterAll(async () => {
    await i18n.changeLanguage(previous);
  });

  it("uses English messages in the English UI", async () => {
    await i18n.changeLanguage("en");
    const messages = [
      ...issueMessages({ websiteUrl: "not a url" }),
      ...issueMessages({ settingsConfig: "" }),
      ...issueMessages({ settingsConfig: "{ bad" }),
      parseJsonError(new Error("not a syntax error")),
      validateJsonConfig("[]"),
      validateJsonConfig("{ bad"),
      updateCommonConfigSnippet("{ bad", "{}", true).error ?? "",
      updateCommonConfigSnippet("{}", "{ bad", true).error ?? "",
    ];
    expect(messages).toContain("Please enter a valid URL");
    expect(messages).toContain("Please fill in configuration content");
    expect(messages).toContain("Config must be a JSON object");
    expect(messages).toContain(
      "Common config snippet: JSON format error, please check the syntax",
    );
    for (const message of messages) {
      expect(message).not.toBe("");
      expect(message).not.toMatch(CJK);
    }
  });

  it("switches to Chinese messages in the Chinese UI", async () => {
    await i18n.changeLanguage("zh");
    expect(issueMessages({ websiteUrl: "not a url" })).toContain(
      "请输入有效的网址",
    );
    expect(issueMessages({ settingsConfig: "" })).toContain("请填写配置内容");
    expect(validateJsonConfig("[]")).toBe("配置必须是 JSON 对象");
    expect(issueMessages({ settingsConfig: "{ bad" })[0]).toMatch(
      /^JSON 格式错误：/,
    );
  });

  it("switches to Japanese messages in the Japanese UI", async () => {
    await i18n.changeLanguage("ja");
    expect(issueMessages({ websiteUrl: "not a url" })).toContain(
      "有効な URL を入力してください",
    );
    expect(validateJsonConfig("{ bad", "X")).toBe(
      "Xの JSON 形式が不正です。構文を確認してください",
    );
  });
});
