import { describe, expect, it } from "vitest";
import { buildMcpDeepLink, buildPromptDeepLink } from "@/utils/shareDeepLinks";
import type { McpServer } from "@/types";

const decode = (b64: string) =>
  new TextDecoder().decode(
    Uint8Array.from(atob(b64), (char) => char.charCodeAt(0)),
  );

const server: McpServer = {
  id: "fetch",
  name: "Fetch",
  server: {
    type: "stdio",
    command: "uvx",
    args: ["mcp-server-fetch"],
    env: { API_KEY: "secret" },
  },
  apps: {
    claude: true,
    codex: true,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
  },
};

describe("buildMcpDeepLink", () => {
  it("encodes the server under mcpServers and strips env by default", () => {
    const { url, envOmitted, apps } = buildMcpDeepLink(server);
    expect(envOmitted).toBe(true);
    expect(apps).toEqual(["claude", "codex"]);
    const params = new URL(url.replace("ccswitch://", "https://x/"))
      .searchParams;
    expect(params.get("resource")).toBe("mcp");
    expect(params.get("name")).toBe("fetch");
    expect(params.get("apps")).toBe("claude,codex");
    const config = JSON.parse(decode(params.get("config")!));
    expect(config.mcpServers.fetch.command).toBe("uvx");
    expect(config.mcpServers.fetch.env).toBeUndefined();
  });

  it("keeps env when asked and falls back to claude when nothing is enabled", () => {
    const { url, envOmitted, apps } = buildMcpDeepLink(
      { ...server, apps: { ...server.apps, claude: false, codex: false } },
      { includeEnv: true },
    );
    expect(envOmitted).toBe(false);
    expect(apps).toEqual(["claude"]);
    const params = new URL(url.replace("ccswitch://", "https://x/"))
      .searchParams;
    const config = JSON.parse(decode(params.get("config")!));
    expect(config.mcpServers.fetch.env).toEqual({ API_KEY: "secret" });
  });
});

describe("buildPromptDeepLink", () => {
  it("base64-encodes unicode content and includes the description", () => {
    const url = buildPromptDeepLink(
      {
        id: "p1",
        name: "代码审查",
        content: "# 角色\n你是审查专家",
        description: " strict ",
        enabled: false,
      },
      "codex",
    );
    const params = new URL(url.replace("ccswitch://", "https://x/"))
      .searchParams;
    expect(params.get("resource")).toBe("prompt");
    expect(params.get("app")).toBe("codex");
    expect(params.get("name")).toBe("代码审查");
    expect(params.get("description")).toBe("strict");
    expect(decode(params.get("content")!)).toBe("# 角色\n你是审查专家");
  });
});
