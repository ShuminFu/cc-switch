import type { ProviderMeta } from "./provider";

// 统一供应商的应用启用状态
export interface UniversalProviderApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
}

// Claude 模型配置
export interface ClaudeModelConfig {
  model?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
}

// Codex 模型配置
export interface CodexModelConfig {
  model?: string;
  reasoningEffort?: string;
}

// Gemini 模型配置
export interface GeminiModelConfig {
  model?: string;
}

// 各应用的模型配置
export interface UniversalProviderModels {
  claude?: ClaudeModelConfig;
  codex?: CodexModelConfig;
  gemini?: GeminiModelConfig;
}

// 统一供应商（跨应用共享配置）
export interface UniversalProvider {
  id: string;
  name: string;
  providerType: string; // "newapi" | "custom" 等
  apps: UniversalProviderApps;
  baseUrl: string;
  apiKey: string;
  models: UniversalProviderModels;
  websiteUrl?: string;
  notes?: string;
  icon?: string;
  iconColor?: string;
  meta?: ProviderMeta;
  createdAt?: number;
  sortIndex?: number;
}

// 统一供应商映射（id -> UniversalProvider）
export type UniversalProvidersMap = Record<string, UniversalProvider>;

// ============================================================================
// OpenCode 专属配置（v3.9.2+）
// ============================================================================
