// OpenClaw 模型配置
export interface OpenClawModel {
  id: string;
  name: string;
  alias?: string;
  reasoning?: boolean; // 是否支持推理模式（如 o1、DeepSeek R1）
  input?: string[]; // 支持的输入类型（如 ["text"]、["text", "image"]）
  cost?: {
    input: number;
    output: number;
    cacheRead?: number; // 缓存读取价格
    cacheWrite?: number; // 缓存写入价格
  };
  contextWindow?: number;
  maxTokens?: number; // 最大输出 token 数
  compat?: {
    maxTokensField?: string; // 最大输出 token 请求字段名（如 "max_tokens"）
  };
}

// OpenClaw 默认模型配置（agents.defaults.model）
export interface OpenClawDefaultModel {
  primary: string;
  fallbacks?: string[];
}

// OpenClaw 模型目录条目（agents.defaults.models 中的值）
export interface OpenClawModelCatalogEntry {
  alias?: string;
}

export interface OpenClawHealthWarning {
  code: string;
  message: string;
  path?: string;
}

export interface OpenClawWriteOutcome {
  backupPath?: string;
  warnings: OpenClawHealthWarning[];
}

export type OpenClawToolsProfile = "minimal" | "coding" | "messaging" | "full";

// OpenClaw 供应商配置（settings_config 结构）
// 对应 OpenClaw 的 models.providers.<provider-id> 配置
export interface OpenClawProviderConfig {
  baseUrl?: string; // API 端点
  apiKey?: string; // API 密钥
  api?: string; // API 协议类型（如 "openai-completions"、"anthropic"）
  models?: OpenClawModel[]; // 可用模型列表
  headers?: Record<string, string>; // 自定义请求头（如 User-Agent）
  authHeader?: boolean; // 供应商自定义认证开关（如 Longcat）
}

// OpenClaw agents.defaults 完整配置
export interface OpenClawAgentsDefaults {
  model?: OpenClawDefaultModel;
  models?: Record<string, OpenClawModelCatalogEntry>;
  timeoutSeconds?: number;
  timeout?: number;
  [key: string]: unknown; // preserve unknown fields
}

// OpenClaw env 配置（openclaw.json 的 env 节点）
export interface OpenClawEnvConfig {
  [key: string]: unknown;
}

// OpenClaw tools 配置（openclaw.json 的 tools 节点）
export interface OpenClawToolsConfig {
  profile?: OpenClawToolsProfile | string;
  allow?: string[];
  deny?: string[];
  [key: string]: unknown; // preserve unknown fields
}

// ============================================================================
// Hermes Agent 专属配置
// ============================================================================
