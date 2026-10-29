import { TFunction } from "i18next";
import type { TranslationKey } from "@/i18n/keys";
import { IpcError, toIpcError, type IpcErrorCode } from "@/lib/api/ipc";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 结构化技能错误（来自后端 `IpcError::skill` 的 `details`）
 */
export interface SkillError {
  code: IpcErrorCode;
  context: Record<string, string>;
  suggestion?: string;
}

/**
 * Skills 领域错误码（与 src-tauri/src/error.rs 的 `ErrorCode` 一致）
 */
const SKILL_ERROR_CODES: ReadonlySet<IpcErrorCode> = new Set<IpcErrorCode>([
  "SKILL_NOT_FOUND",
  "INVALID_SKILL_DIRECTORY",
  "SKILL_DIRECTORY_CONFLICT",
  "SKILL_DIR_NOT_FOUND",
  "DOWNLOAD_TIMEOUT",
  "DOWNLOAD_FAILED",
  "EMPTY_ARCHIVE",
  "NO_SKILLS_IN_ZIP",
]);

/**
 * 从 invoke 的拒绝值中取出技能错误；按 `code` 判断，不解析文案。
 * 非 Skills 领域错误返回 null
 */
export function parseSkillError(error: unknown): SkillError | null {
  const normalized = toIpcError(error);
  if (!(normalized instanceof IpcError)) return null;
  if (!SKILL_ERROR_CODES.has(normalized.code)) return null;

  const details = (normalized.details ?? {}) as {
    context?: Record<string, string> | null;
    suggestion?: string | null;
  };
  return {
    code: normalized.code,
    context: details.context ?? {},
    suggestion: details.suggestion ?? undefined,
  };
}

/**
 * 错误码 -> i18n key
 */
const SKILL_ERROR_KEYS = {
  SKILL_NOT_FOUND: "skills.error.skillNotFound",
  DOWNLOAD_TIMEOUT: "skills.error.downloadTimeout",
  DOWNLOAD_FAILED: "skills.error.downloadFailed",
  SKILL_DIR_NOT_FOUND: "skills.error.skillDirNotFound",
  SKILL_DIRECTORY_CONFLICT: "skills.error.directoryConflict",
  EMPTY_ARCHIVE: "skills.error.emptyArchive",
  NO_SKILLS_IN_ZIP: "skills.error.noSkillsInZip",
} as const satisfies Partial<Record<IpcErrorCode, TranslationKey>>;

type SkillErrorKey =
  | (typeof SKILL_ERROR_KEYS)[keyof typeof SKILL_ERROR_KEYS]
  | "skills.error.unknownError";

/**
 * 将错误码映射到 i18n key
 */
function getErrorI18nKey(code: IpcErrorCode): SkillErrorKey {
  const mapping: Partial<Record<IpcErrorCode, SkillErrorKey>> =
    SKILL_ERROR_KEYS;
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
 * @param error invoke 的拒绝值（`IpcError` 或尚未迁移的字符串错误）
 * @param t i18next 翻译函数
 * @param defaultTitle 默认标题的 i18n key（如 "skills.installFailed"）
 * @returns 包含标题和描述的对象
 */
export function formatSkillError(
  error: unknown,
  t: TFunction,
  defaultTitle: TranslationKey = "skills.installFailed",
): { title: string; description: string } {
  const parsedError = parseSkillError(error);

  if (!parsedError) {
    // 不是技能错误：返回原始错误文案
    return {
      title: t(defaultTitle),
      description: extractErrorMessage(error) || t("common.error"),
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
