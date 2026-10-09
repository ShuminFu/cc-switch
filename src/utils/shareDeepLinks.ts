import type { McpServer, McpApps } from "@/types";
import type { Prompt } from "@/lib/api/prompts";
import type { AppId } from "@/lib/api/types";
import { encodeDeepLinkConfig } from "@/utils/providerDeepLink";

/** 深链接 `apps` 参数接受的应用 id（与后端 parse_mcp_apps 一致） */
const MCP_LINK_APPS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
];

export interface McpDeepLinkOptions {
  /** 把 server.env 一并写进链接（可能含密钥，默认不带） */
  includeEnv?: boolean;
}

export interface McpDeepLinkResult {
  url: string;
  /** 为 true 表示原配置含 env 但链接里已去掉 */
  envOmitted: boolean;
  apps: AppId[];
}

const enabledApps = (apps: McpApps | undefined): AppId[] =>
  MCP_LINK_APPS.filter((app) => Boolean(apps?.[app as keyof McpApps]));

/**
 * 生成 `ccswitch://v1/import?resource=mcp&…`：name = 服务器 id，
 * config = base64({"mcpServers": {id: spec}})，与导入端一一对应。
 */
export function buildMcpDeepLink(
  server: McpServer,
  options: McpDeepLinkOptions = {},
): McpDeepLinkResult {
  const spec = { ...(server.server as Record<string, unknown>) };
  const hasEnv =
    spec.env != null &&
    typeof spec.env === "object" &&
    Object.keys(spec.env as Record<string, unknown>).length > 0;
  const envOmitted = hasEnv && !options.includeEnv;
  if (envOmitted) {
    delete spec.env;
  }
  const apps = enabledApps(server.apps);
  const linkApps = apps.length > 0 ? apps : (["claude"] as AppId[]);
  const params = new URLSearchParams({
    resource: "mcp",
    name: server.id,
    apps: linkApps.join(","),
    config: encodeDeepLinkConfig(
      JSON.stringify({ mcpServers: { [server.id]: spec } }),
    ),
  });
  return {
    url: `ccswitch://v1/import?${params.toString()}`,
    envOmitted,
    apps: linkApps,
  };
}

/** 生成 `ccswitch://v1/import?resource=prompt&…`，content 为 base64 */
export function buildPromptDeepLink(prompt: Prompt, app: AppId): string {
  const params = new URLSearchParams({
    resource: "prompt",
    app,
    name: prompt.name,
    content: encodeDeepLinkConfig(prompt.content),
  });
  if (prompt.description?.trim()) {
    params.set("description", prompt.description.trim());
  }
  return `ccswitch://v1/import?${params.toString()}`;
}
