import { invoke, call } from "./invoke";
import { commands, type JsonValue } from "@/lib/bindings";
import type {
  McpConfigResponse,
  McpServer,
  McpServerSpec,
  McpServersMap,
  McpStatus,
} from "@/types";
import type { AppId } from "./types";

export const mcpApi = {
  async getStatus(): Promise<McpStatus> {
    return await invoke("get_claude_mcp_status");
  },

  async readConfig(): Promise<string | null> {
    return await call(commands.readClaudeMcpConfig());
  },

  async upsertServer(
    id: string,
    spec: McpServerSpec | Record<string, any>,
  ): Promise<boolean> {
    return await call(commands.upsertClaudeMcpServer(id, spec));
  },

  async deleteServer(id: string): Promise<boolean> {
    return await call(commands.deleteClaudeMcpServer(id));
  },

  async validateCommand(cmd: string): Promise<boolean> {
    return await call(commands.validateMcpCommand(cmd));
  },

  /**
   * @deprecated 使用 getAllServers() 代替（v3.7.0+）
   */
  async getConfig(app: AppId = "claude"): Promise<McpConfigResponse> {
    return await invoke("get_mcp_config", { app });
  },

  /**
   * @deprecated 使用 upsertUnifiedServer() 代替（v3.7.0+）
   */
  async upsertServerInConfig(
    app: AppId,
    id: string,
    spec: McpServer,
    options?: { syncOtherSide?: boolean },
  ): Promise<boolean> {
    return await call(
      commands.upsertMcpServerInConfig(
        app,
        id,
        spec as unknown as JsonValue,
        options?.syncOtherSide ?? null,
      ),
    );
  },

  /**
   * @deprecated 使用 deleteUnifiedServer() 代替（v3.7.0+）
   */
  async deleteServerInConfig(
    app: AppId,
    id: string,
    options?: { syncOtherSide?: boolean },
  ): Promise<boolean> {
    // The backend deletes the server from every app; `syncOtherSide` is kept
    // in the signature for callers but has no effect.
    void options;
    return await call(commands.deleteMcpServerInConfig(app, id));
  },

  /**
   * @deprecated 使用 toggleApp() 代替（v3.7.0+）
   */
  async setEnabled(app: AppId, id: string, enabled: boolean): Promise<boolean> {
    return await invoke("set_mcp_enabled", { app, id, enabled });
  },

  // ========================================================================
  // v3.7.0 新增：统一 MCP 管理 API
  // ========================================================================

  /**
   * 获取所有 MCP 服务器（统一结构）
   */
  async getAllServers(): Promise<McpServersMap> {
    return await invoke("get_mcp_servers");
  },

  /**
   * 添加或更新 MCP 服务器（统一结构）
   */
  async upsertUnifiedServer(server: McpServer): Promise<void> {
    return await invoke("upsert_mcp_server", { server });
  },

  /**
   * 删除 MCP 服务器
   */
  async deleteUnifiedServer(id: string): Promise<boolean> {
    return await call(commands.deleteMcpServer(id));
  },

  /**
   * 切换 MCP 服务器在指定应用的启用状态
   */
  async toggleApp(
    serverId: string,
    app: AppId,
    enabled: boolean,
  ): Promise<void> {
    return await call(commands.toggleMcpApp(serverId, app, enabled));
  },

  /**
   * 从所有应用导入 MCP 服务器
   */
  async importFromApps(): Promise<number> {
    return await call(commands.importMcpFromApps());
  },
};
