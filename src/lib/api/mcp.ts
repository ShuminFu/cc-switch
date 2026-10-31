import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type {
  McpServer,
  McpServerSpec,
  McpServersMap,
  McpStatus,
} from "@/types";
import type { AppId } from "./types";
import type { McpSyncSkip } from "@/lib/bindings";

export const mcpApi = {
  async getStatus(): Promise<McpStatus> {
    return await call(commands.getClaudeMcpStatus());
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
   * Resolves to the enabled apps whose config directory is missing (nothing
   * was written for them).
   */
  async upsertUnifiedServer(server: McpServer): Promise<McpSyncSkip[]> {
    return (
      (await invoke<McpSyncSkip[] | null>("upsert_mcp_server", { server })) ??
      []
    );
  },

  /** MCP-capable apps whose config directory is missing (CFG-46). */
  async getSyncSkips(): Promise<McpSyncSkip[]> {
    return await call(commands.getMcpSyncSkips());
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
  ): Promise<McpSyncSkip[]> {
    return (await call(commands.toggleMcpApp(serverId, app, enabled))) ?? [];
  },

  /**
   * 从所有应用导入 MCP 服务器
   */
  async importFromApps(): Promise<number> {
    return await call(commands.importMcpFromApps());
  },
};
