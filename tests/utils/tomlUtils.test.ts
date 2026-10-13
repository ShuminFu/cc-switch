import { describe, expect, it } from "vitest";
import {
  extractIdFromToml,
  mcpServerToToml,
  tomlToMcpServer,
  validateToml,
} from "@/utils/tomlUtils";
import { normalizeQuotes } from "@/utils/textNormalization";

describe("normalizeQuotes", () => {
  it("maps curly and full-width quotes to ASCII", () => {
    expect(normalizeQuotes("“a” „b‟ ＂c＂ ‘d’ ＇e＇")).toBe(
      `"a" "b" "c" 'd' 'e'`,
    );
  });

  it("leaves CJK corner brackets untouched", () => {
    expect(normalizeQuotes("「引用」《书名》")).toBe("「引用」《书名》");
  });
});

describe("validateToml", () => {
  it("treats blank input as valid", () => {
    expect(validateToml("   \n")).toBe("");
  });

  it("accepts TOML typed with Chinese IME quotes", () => {
    expect(validateToml("command = “npx”")).toBe("");
  });

  it("returns the parser message for malformed input", () => {
    const err = validateToml("command = ");
    expect(err).not.toBe("");
    expect(err).not.toBe("mustBeObject");
  });
});

describe("tomlToMcpServer", () => {
  it("rejects empty input", () => {
    expect(() => tomlToMcpServer("  ")).toThrow("TOML 内容不能为空");
  });

  it("parses a bare stdio config, defaulting type to stdio", () => {
    expect(
      tomlToMcpServer(
        'command = "npx"\nargs = ["-y", 1]\n[env]\nPORT = 8080\nNAME = "x"',
      ),
    ).toEqual({
      type: "stdio",
      command: "npx",
      args: ["-y", "1"],
      env: { PORT: "8080", NAME: "x" },
    });
  });

  it("takes the first server from [mcp_servers.<id>]", () => {
    const server = tomlToMcpServer(
      '[mcp_servers.first]\ncommand = "a"\n[mcp_servers.second]\ncommand = "b"',
    );
    expect(server.command).toBe("a");
  });

  it("tolerates the mistaken [mcp.servers.<id>] form", () => {
    const server = tomlToMcpServer(
      '[mcp.servers.fetch]\ntype = "http"\nurl = "https://x.dev/mcp"\n[mcp.servers.fetch.headers]\nAuthorization = "Bearer t"',
    );
    expect(server).toEqual({
      type: "http",
      url: "https://x.dev/mcp",
      headers: { Authorization: "Bearer t" },
    });
  });

  it("preserves unknown extension fields like timeout_ms", () => {
    const server = tomlToMcpServer('command = "npx"\ntimeout_ms = 30000');
    expect(server.timeout_ms).toBe(30000);
  });

  it("requires command for stdio and url for http/sse", () => {
    expect(() => tomlToMcpServer('type = "stdio"\nargs = []')).toThrow(
      "command",
    );
    expect(() => tomlToMcpServer('type = "sse"\ncommand = "x"')).toThrow("url");
  });

  it("rejects unsupported types", () => {
    expect(() => tomlToMcpServer('type = "websocket"\nurl = "ws://x"')).toThrow(
      "websocket",
    );
  });

  it("rejects documents with no recognizable server", () => {
    expect(() => tomlToMcpServer('name = "nothing"')).toThrow(
      "无法识别的 TOML 格式",
    );
    expect(() => tomlToMcpServer("[mcp_servers]")).toThrow(
      "无法识别的 TOML 格式",
    );
  });

  it("round-trips through mcpServerToToml", () => {
    const original = {
      type: "stdio" as const,
      command: "uvx",
      args: ["mcp-server-fetch"],
      env: { KEY: 'va"lue' },
    };
    expect(tomlToMcpServer(mcpServerToToml(original))).toEqual(original);
  });
});

describe("mcpServerToToml", () => {
  it("drops undefined fields", () => {
    const toml = mcpServerToToml({
      type: "stdio",
      command: "npx",
      cwd: undefined,
    });
    expect(toml).not.toContain("cwd");
    expect(toml).toBe(toml.trim());
  });
});

describe("extractIdFromToml", () => {
  it("uses the table key from [mcp_servers.<id>]", () => {
    expect(extractIdFromToml('[mcp_servers.my-tool]\ncommand = "x"')).toBe(
      "my-tool",
    );
  });

  it("uses the table key from [mcp.servers.<id>]", () => {
    expect(extractIdFromToml('[mcp.servers.legacy]\ncommand = "x"')).toBe(
      "legacy",
    );
  });

  it("derives an id from the command basename, stripping script extensions", () => {
    expect(extractIdFromToml('command = "C:\\\\tools\\\\my-server.EXE"')).toBe(
      "my-server",
    );
    expect(extractIdFromToml('command = "/usr/local/bin/server.py"')).toBe(
      "server",
    );
  });

  it("returns empty string on parse failure or no hint", () => {
    expect(extractIdFromToml("this is = = broken")).toBe("");
    expect(extractIdFromToml('url = "https://x"')).toBe("");
  });
});
