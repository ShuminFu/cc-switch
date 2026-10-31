import { invoke, call } from "./invoke";
import { commands, type JsonValue } from "@/lib/bindings";
import type {
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
    spec: McpServerSpec | Record<string, unknown>,
  ): Promise<boolean> {
    // The spec is plain JSON from the MCP form (FE-ARCH-28).
    return await call(commands.upsertClaudeMcpServer(id, spec as JsonValue));
  },

  async deleteServer(id: string): Promise<boolean> {
    return await call(commands.deleteClaudeMcpServer(id));
  },

  async validateCommand(cmd: string): Promise<boolean> {
    return await call(commands.validateMcpCommand(cmd));
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
