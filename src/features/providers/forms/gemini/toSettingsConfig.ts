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

/**
 * Gemini 只管理通用配置开关；其余应用专属 meta 字段写 undefined。
 * 例外：Anthropic 上游（`apiFormat = anthropic`）的路由字段表单不编辑，沿用已有值，
 * 否则编辑一次就会关掉该转换
 */
export const toGeminiAppMeta = ({
  commonConfigEnabled,
  baseMeta,
}: {
  commonConfigEnabled: boolean;
  baseMeta?: Pick<
    AppProviderMeta,
    "apiFormat" | "apiKeyField" | "maxOutputTokens"
  >;
}): AppProviderMeta => ({
  commonConfigEnabled,
  codexChatReasoning: undefined,
  promptCacheRouting: undefined,
  customUserAgent: undefined,
  localProxyRequestOverrides: undefined,
  apiFormat: baseMeta?.apiFormat,
  apiKeyField: baseMeta?.apiKeyField,
  impersonateClaudeCode: undefined,
  maxOutputTokens: baseMeta?.maxOutputTokens,
  isFullUrl: undefined,
});
