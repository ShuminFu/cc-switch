/**
 * 各应用供应商 settingsConfig 的读取方式（单一来源）
 *
 * 每个应用的 settingsConfig 结构不同（env / auth+TOML / 扁平字段 / SDK options），
 * 以前提交表单、用量脚本、供应商卡片各自用 `appId === "x"` 分支解析一遍。
 * 这里按 AppId 建表：新增应用时编译器会要求补齐凭据读取方式。
 */

import type { AppId } from "@/lib/api/types";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
} from "@/utils/providerConfigUtils";
import { parseGrokBuildConfig } from "@/utils/grokBuildConfig";

export interface ProviderCredentials {
  apiKey: string | undefined;
  baseUrl: string | undefined;
}

type SettingsConfig = Record<string, unknown>;

/** settingsConfig 中的嵌套对象（env / auth / options）；缺失或非对象时视为空对象 */
const section = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" ? (value as Record<string, unknown>) : {};

const text = (value: unknown) => value as string | undefined;

const anthropicEnvCredentials = (
  config: SettingsConfig,
): ProviderCredentials => {
  // Claude / Claude Desktop: { env: { ANTHROPIC_AUTH_TOKEN | ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL } }
  // Key fallbacks mirror the backend resolver (Provider::resolve_usage_credentials).
  const env = section(config.env);
  return {
    apiKey: text(
      env.ANTHROPIC_AUTH_TOKEN ||
        env.ANTHROPIC_API_KEY ||
        env.OPENROUTER_API_KEY ||
        env.GOOGLE_API_KEY,
    ),
    baseUrl: text(env.ANTHROPIC_BASE_URL),
  };
};

const PROVIDER_CREDENTIALS: Record<
  AppId,
  (config: SettingsConfig) => ProviderCredentials
> = {
  claude: anthropicEnvCredentials,
  "claude-desktop": anthropicEnvCredentials,
  codex: (config) => {
    // Codex: { auth: { OPENAI_API_KEY }, config: TOML string with base_url }
    const auth = section(config.auth);
    const configToml = text(config.config);
    const apiKey =
      typeof auth.OPENAI_API_KEY === "string" && auth.OPENAI_API_KEY.trim()
        ? auth.OPENAI_API_KEY
        : extractCodexExperimentalBearerToken(configToml);
    return { apiKey, baseUrl: extractCodexBaseUrl(configToml) };
  },
  gemini: (config) => {
    // Gemini: { env: { GEMINI_API_KEY, GOOGLE_GEMINI_BASE_URL } }
    // Key fallback mirrors the backend resolver (Provider::resolve_usage_credentials).
    const env = section(config.env);
    return {
      apiKey: text(env.GEMINI_API_KEY || env.GOOGLE_API_KEY),
      baseUrl: text(env.GOOGLE_GEMINI_BASE_URL),
    };
  },
  grokbuild: (config) => {
    const grokConfig = parseGrokBuildConfig(text(config.config));
    return { apiKey: grokConfig.apiKey, baseUrl: grokConfig.baseUrl };
  },
  // OpenCode (OMO): 凭据嵌在 options.{baseURL, apiKey}（SDK options 对象）
  opencode: (config) => {
    const options = section(config.options);
    return { apiKey: text(options.apiKey), baseUrl: text(options.baseURL) };
  },
  // OpenClaw: settingsConfig 顶层扁平（camelCase，对应 openclaw.json）
  openclaw: (config) => ({
    apiKey: text(config.apiKey),
    baseUrl: text(config.baseUrl),
  }),
  // Hermes: settingsConfig 顶层扁平（snake_case，对应 config.yaml）
  hermes: (config) => ({
    apiKey: text(config.api_key),
    baseUrl: text(config.base_url),
  }),
};

/** 从供应商 settingsConfig 中读取 API Key 与 Base URL（原样，不做裁剪） */
export function extractProviderCredentials(
  appId: AppId,
  settingsConfig: SettingsConfig,
): ProviderCredentials {
  return (
    PROVIDER_CREDENTIALS[appId]?.(settingsConfig) ?? {
      apiKey: undefined,
      baseUrl: undefined,
    }
  );
}

const isBlank = (value: unknown) =>
  !value || (typeof value === "string" && value.trim() === "");

/**
 * 支持官方登录（OAuth / 订阅）的应用：settingsConfig 未填自定义地址或密钥时
 * 即直连官方 API。
 */
const OFFICIAL_LOGIN_CHECKS: Partial<
  Record<AppId, (config: SettingsConfig | undefined) => boolean>
> = {
  claude: (config) => isBlank(section(config?.env).ANTHROPIC_BASE_URL),
  // 无 OPENAI_API_KEY → 使用 Codex CLI 内置 OAuth（官方）
  codex: (config) => {
    const bearerToken =
      typeof config?.config === "string"
        ? extractCodexExperimentalBearerToken(config.config)
        : undefined;
    return !bearerToken && isBlank(section(config?.auth).OPENAI_API_KEY);
  },
  // 无 GEMINI_API_KEY 且无 GOOGLE_GEMINI_BASE_URL → Google OAuth 官方模式
  gemini: (config) => {
    const env = section(config?.env);
    return isBlank(env.GEMINI_API_KEY) && isBlank(env.GOOGLE_GEMINI_BASE_URL);
  },
};

/** 应用是否支持官方登录（官方订阅用量查询等） */
export function supportsOfficialLogin(appId: AppId): boolean {
  return OFFICIAL_LOGIN_CHECKS[appId] !== undefined;
}

/** settingsConfig 是否为官方登录配置；不支持官方登录的应用恒为 false */
export function isOfficialLoginConfig(
  appId: AppId,
  settingsConfig: unknown,
): boolean {
  return (
    OFFICIAL_LOGIN_CHECKS[appId]?.(
      settingsConfig as SettingsConfig | undefined,
    ) ?? false
  );
}
