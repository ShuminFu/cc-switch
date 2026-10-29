import { TFunction } from "i18next";
import type { TranslationKey } from "@/i18n/keys";

/**
 * 结构化错误对象
 */
export interface SkillError {
  code: string;
  context: Record<string, string>;
  suggestion?: string;
}

/**
 * 尝试解析后端返回的错误字符串
 * 如果是 JSON 格式，返回结构化错误；否则返回 null
 */
export function parseSkillError(errorString: string): SkillError | null {
  try {
    const parsed = JSON.parse(errorString);
    if (parsed.code && parsed.context) {
      return parsed as SkillError;
    }
  } catch {
    // 不是 JSON 格式，返回 null
  }
  return null;
}

/**
 * 错误码 -> i18n key
 */
const SKILL_ERROR_KEYS = {
  SKILL_NOT_FOUND: "skills.error.skillNotFound",
  MISSING_REPO_INFO: "skills.error.missingRepoInfo",
  DOWNLOAD_TIMEOUT: "skills.error.downloadTimeout",
  DOWNLOAD_FAILED: "skills.error.downloadFailed",
  SKILL_DIR_NOT_FOUND: "skills.error.skillDirNotFound",
  SKILL_DIRECTORY_CONFLICT: "skills.error.directoryConflict",
  EMPTY_ARCHIVE: "skills.error.emptyArchive",
  GET_HOME_DIR_FAILED: "skills.error.getHomeDirFailed",
  NO_SKILLS_IN_ZIP: "skills.error.noSkillsInZip",
} as const satisfies Record<string, TranslationKey>;

type SkillErrorKey =
  | (typeof SKILL_ERROR_KEYS)[keyof typeof SKILL_ERROR_KEYS]
  | "skills.error.unknownError";

/**
 * 将错误码映射到 i18n key
 */
function getErrorI18nKey(code: string): SkillErrorKey {
  const mapping: Record<string, SkillErrorKey> = SKILL_ERROR_KEYS;
  return mapping[code] || "skills.error.unknownError";
}

/**
 * 将建议码映射到 i18n key
 */
function getSuggestionI18nKey(suggestion: string): TranslationKey {
  const mapping: Record<string, TranslationKey> = {
    checkNetwork: "skills.error.suggestion.checkNetwork",
    checkProxy: "skills.error.suggestion.checkProxy",
    retryLater: "skills.error.suggestion.retryLater",
    checkRepoUrl: "skills.error.suggestion.checkRepoUrl",
    checkPermission: "skills.error.suggestion.checkPermission",
    uninstallFirst: "skills.error.suggestion.uninstallFirst",
    checkZipContent: "skills.error.suggestion.checkZipContent",
    http403: "skills.error.http403",
    http404: "skills.error.http404",
    http429: "skills.error.http429",
  };

  // 未登记的建议码原样作为 key 传给 t()，缺失时 i18next 会直接返回原文
  return mapping[suggestion] || (suggestion as TranslationKey);
}

/**
 * 格式化技能错误为用户友好的消息
 * @param errorString 后端返回的错误字符串
 * @param t i18next 翻译函数
 * @param defaultTitle 默认标题的 i18n key（如 "skills.installFailed"）
 * @returns 包含标题和描述的对象
 */
export function formatSkillError(
  errorString: string,
  t: TFunction,
  defaultTitle: TranslationKey = "skills.installFailed",
): { title: string; description: string } {
  const parsedError = parseSkillError(errorString);

  if (!parsedError) {
    // 如果不是结构化错误，返回原始错误字符串
    return {
      title: t(defaultTitle),
      description: errorString || t("common.error"),
    };
  }

  const { code, context, suggestion } = parsedError;

  // 获取错误消息的 i18n key
  const errorKey = getErrorI18nKey(code);

  // 构建描述（错误消息 + 建议）
  // context 是插值变量表；声明为不含 i18next 的 context 选项，避免类型推导去匹配 key_<context> 变体
  let description = t(
    errorKey,
    context as Record<string, string> & { context?: undefined },
  );

  // 如果有建议，追加到描述中
  if (suggestion) {
    const suggestionKey = getSuggestionI18nKey(suggestion);
    const suggestionText = t(suggestionKey);
    description += `\n\n${suggestionText}`;
  }

  return {
    title: t(defaultTitle),
    description,
  };
}
