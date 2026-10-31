import { invoke } from "./invoke";

export interface FetchedModel {
  id: string;
  ownedBy: string | null;
}

/**
 * 从供应商获取可用模型列表
 *
 * 使用 OpenAI 兼容的 GET /v1/models 端点。优先用 `modelsUrl` 精确覆写；
 * 否则后端会对 baseURL 生成候选列表并按序尝试（含"剥离 /anthropic 等兼容子路径"兜底）。
 */
export async function fetchModelsForConfig(
  baseUrl: string,
  apiKey: string,
  isFullUrl?: boolean,
  modelsUrl?: string,
  customUserAgent?: string,
): Promise<FetchedModel[]> {
  return invoke("fetch_models_for_config", {
    baseUrl,
    apiKey,
    isFullUrl,
    modelsUrl,
    customUserAgent,
  });
}

/**
 * 获取 Codex OAuth (ChatGPT Plus/Pro 反代) 可用模型列表
 *
 * Codex OAuth 使用 ChatGPT 的 backend-api/codex 端点，不兼容普通 /v1/models。
 */
export async function fetchCodexOauthModels(
  accountId?: string | null,
): Promise<FetchedModel[]> {
  return invoke("get_codex_oauth_models", {
    accountId: accountId || null,
  });
}
