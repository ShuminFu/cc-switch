// MCP 服务器连接参数（宽松：允许扩展字段）
export interface McpServerSpec {
  // 可选：社区常见 .mcp.json 中 stdio 配置可不写 type
  type?: "stdio" | "http" | "sse";
  // stdio 字段
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  // http 和 sse 字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  [key: string]: any;
}

// v3.7.0: MCP 服务器应用启用状态
export interface McpApps {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild?: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

// MCP 服务器条目（v3.7.0 统一结构）
export interface McpServer {
  id: string;
  name: string;
  server: McpServerSpec;
  apps: McpApps; // v3.7.0: 标记应用到哪些客户端
  description?: string;
  tags?: string[];
  homepage?: string;
  docs?: string;
  // 兼容旧字段（v3.6.x 及以前）
  enabled?: boolean; // 已废弃，v3.7.0 使用 apps 字段
  source?: string;
  [key: string]: any;
}

// MCP 服务器映射（id -> McpServer）
export type McpServersMap = Record<string, McpServer>;

// MCP 配置状态
export interface McpStatus {
  userConfigPath: string;
  userConfigExists: boolean;
  serverCount: number;
}

// 新：来自 config.json 的 MCP 列表响应
export interface McpConfigResponse {
  configPath: string;
  servers: Record<string, McpServer>;
}

// ============================================================================
// 统一供应商（Universal Provider）- 跨应用共享配置
// ============================================================================
