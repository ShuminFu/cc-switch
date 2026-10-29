import type { AppProviderMeta } from "../coreForm";

/** 把 .env 文本解析成对象：跳过空行与 # 注释，键和值去掉首尾空白 */
export const geminiEnvToObject = (
  envString: string,
): Record<string, string> => {
  const env: Record<string, string> = {};
  envString.split("\n").forEach((line) => {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) return;
    const equalIndex = trimmed.indexOf("=");
    if (equalIndex > 0) {
      const key = trimmed.substring(0, equalIndex).trim();
      const value = trimmed.substring(equalIndex + 1).trim();
      env[key] = value;
    }
  });
  return env;
};

/**
 * Gemini 的 settingsConfig：`{ env, config }`。config 是已解析的扩展配置
 * （解析失败时调用方必须先阻止保存）。
 */
export const toGeminiSettingsConfig = ({
  env,
  config,
}: {
  env: string;
  config: Record<string, unknown>;
}): string => JSON.stringify({ env: geminiEnvToObject(env), config });

/** Gemini 只管理通用配置开关；其余应用专属 meta 字段写 undefined */
export const toGeminiAppMeta = ({
  commonConfigEnabled,
}: {
  commonConfigEnabled: boolean;
}): AppProviderMeta => ({
  commonConfigEnabled,
  codexChatReasoning: undefined,
  promptCacheRouting: undefined,
  customUserAgent: undefined,
  localProxyRequestOverrides: undefined,
  apiFormat: undefined,
  apiKeyField: undefined,
  impersonateClaudeCode: undefined,
  maxOutputTokens: undefined,
  isFullUrl: undefined,
});
