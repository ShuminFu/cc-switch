import { describe, expect, it } from "vitest";
import {
  extractErrorMessage,
  translateMcpBackendError,
} from "@/utils/errorUtils";

describe("extractErrorMessage", () => {
  it.each([
    [null, ""],
    [undefined, ""],
    ["", ""],
    ["plain string", "plain string"],
    [new Error("boom"), "boom"],
    [{ message: "from message" }, "from message"],
    [{ error: "from error" }, "from error"],
    [{ detail: "from detail" }, "from detail"],
    [{ payload: "from payload" }, "from payload"],
    [{ payload: { detail: "nested detail" } }, "nested detail"],
    [{ message: 42 }, ""],
    [42, ""],
  ])("extracts from %o", (input, expected) => {
    expect(extractErrorMessage(input)).toBe(expected);
  });

  it("skips whitespace-only messages and falls through to payload", () => {
    expect(extractErrorMessage({ message: "  ", payload: "real" })).toBe(
      "real",
    );
  });

  it("falls back to object fields when an Error has an empty message", () => {
    const err = Object.assign(new Error(""), { detail: "attached detail" });
    expect(extractErrorMessage(err)).toBe("attached detail");
  });

  it("skips a blank message field in favour of error/detail", () => {
    expect(extractErrorMessage({ message: "", error: "real error" })).toBe(
      "real error",
    );
    expect(
      extractErrorMessage({ payload: { message: " ", detail: "inner" } }),
    ).toBe("inner");
  });
});

describe("translateMcpBackendError", () => {
  const t = (key: string) => `t:${key}`;

  it("returns empty string for empty or unknown messages", () => {
    expect(translateMcpBackendError("", t)).toBe("");
    expect(translateMcpBackendError("something else", t)).toBe("");
  });

  it.each([
    ["MCP 服务器 ID 不能为空", "t:mcp.error.idRequired"],
    ["MCP 服务器连接定义必须为 JSON 对象", "t:mcp.error.jsonInvalid"],
    [
      "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'",
      "t:mcp.error.jsonInvalid",
    ],
    ["stdio 类型的 MCP 服务器缺少 command 字段", "t:mcp.error.commandRequired"],
    // frontend tomlUtils wording
    [
      "stdio 类型的 MCP 服务器必须包含 command 字段",
      "t:mcp.error.commandRequired",
    ],
    ["sse 类型的 MCP 服务器缺少 url 字段", "t:mcp.wizard.urlRequired"],
    ["URL 不能为空", "t:mcp.wizard.urlRequired"],
    ["无法识别的 TOML 格式。请提供…", "t:mcp.error.tomlInvalid"],
    ["序列化 config.toml 失败: x", "t:mcp.error.tomlInvalid"],
  ])("maps %s", (message, expected) => {
    expect(translateMcpBackendError(`  ${message}  `, t)).toBe(expected);
  });
});
