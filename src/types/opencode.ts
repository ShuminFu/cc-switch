// OpenCode 模型配置
export interface OpenCodeModel {
  name: string;
  limit?: {
    context?: number;
    output?: number;
  };
  options?: Record<string, unknown>; // 模型级别额外选项（provider 路由等）
  // 支持任意额外字段（cost、modalities、thinking、variants 等）
  [key: string]: unknown;
}

// OpenCode 供应商选项
export interface OpenCodeProviderOptions {
  baseURL?: string;
  apiKey?: string;
  headers?: Record<string, string>;
  // 支持额外选项（timeout, setCacheKey 等）
  [key: string]: unknown;
}

// OpenCode 供应商配置（settings_config 结构）
export interface OpenCodeProviderConfig {
  npm: string; // AI SDK 包名，如 "@ai-sdk/openai-compatible"
  name?: string; // 供应商显示名称
  options: OpenCodeProviderOptions;
  models: Record<string, OpenCodeModel>;
}

// OpenCode MCP 服务器配置（与统一格式不同）
export interface OpenCodeMcpServerSpec {
  type: "local" | "remote";
  // local 类型字段
  command?: string[]; // 与统一格式不同：命令和参数合并为数组
  environment?: Record<string, string>; // 与统一格式不同：使用 environment 而非 env
  // remote 类型字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  enabled?: boolean;
}

// ============================================================================
// OpenClaw 专属配置（v3.11.0+）
// ============================================================================
