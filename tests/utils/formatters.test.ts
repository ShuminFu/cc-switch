import { describe, expect, it } from "vitest";
import { formatJSON, parseSmartMcpJson } from "@/utils/formatters";

describe("formatJSON", () => {
  it("returns empty string for blank input", () => {
    expect(formatJSON("  \n ")).toBe("");
  });

  it("pretty prints with two-space indent", () => {
    expect(formatJSON('{"a":[1,{"b":"中"}]}')).toBe(
      '{\n  "a": [\n    1,\n    {\n      "b": "中"\n    }\n  ]\n}',
    );
  });

  it("throws on malformed JSON", () => {
    expect(() => formatJSON("{a:1}")).toThrow();
  });
});

describe("parseSmartMcpJson", () => {
  it("returns an empty config for blank input", () => {
    expect(parseSmartMcpJson("   ")).toEqual({
      config: {},
      formattedConfig: "",
    });
  });

  it("keeps a plain server config as-is", () => {
    const result = parseSmartMcpJson('{"command":"npx","args":["-y"]}');
    expect(result.id).toBeUndefined();
    expect(result.config).toEqual({ command: "npx", args: ["-y"] });
  });

  it("unwraps a single-key wrapper object", () => {
    const result = parseSmartMcpJson('{"fetch":{"command":"uvx"}}');
    expect(result.id).toBe("fetch");
    expect(result.config).toEqual({ command: "uvx" });
    expect(result.formattedConfig).toBe('{\n  "command": "uvx"\n}');
  });

  it("wraps a bare key/value fragment pasted from a larger file", () => {
    const result = parseSmartMcpJson('  "my-server": {"command":"node"}  ');
    expect(result.id).toBe("my-server");
    expect(result.config).toEqual({ command: "node" });
  });

  it("does not unwrap when the single value is an array or primitive", () => {
    expect(parseSmartMcpJson('{"args":["a"]}').id).toBeUndefined();
    expect(parseSmartMcpJson('{"command":"npx"}').id).toBeUndefined();
  });

  it("does not unwrap multi-key objects", () => {
    expect(
      parseSmartMcpJson('{"a":{"command":"x"},"b":{"command":"y"}}').id,
    ).toBeUndefined();
  });

  it("throws on malformed JSON", () => {
    expect(() => parseSmartMcpJson('{"command": }')).toThrow();
  });
});
