import {
  invoke as tauriInvoke,
  type InvokeArgs,
  type InvokeOptions,
} from "@tauri-apps/api/core";

/**
 * 后端 `ErrorCode`（src-tauri/src/error.rs）的线上字符串。
 * 新增变体时两边同步更新；前端按 code 分支，不再匹配错误文案。
 */
export const IPC_ERROR_CODES = [
  "UNKNOWN",
  "CONFIG",
  "INVALID_INPUT",
  "IO",
  "JSON",
  "TOML",
  "LOCK",
  "MCP_VALIDATION",
  "HTTP_STATUS",
  "LOCALIZED",
  "DATABASE",
  "OMO_CONFIG_NOT_FOUND",
  "CIRCUIT_OPEN",
  "NO_PROVIDERS_CONFIGURED",
  "UNSUPPORTED",
  "SKILL_NOT_FOUND",
  "INVALID_SKILL_DIRECTORY",
  "SKILL_DIRECTORY_CONFLICT",
  "SKILL_DIR_NOT_FOUND",
  "DOWNLOAD_TIMEOUT",
  "DOWNLOAD_FAILED",
  "EMPTY_ARCHIVE",
  "NO_SKILLS_IN_ZIP",
] as const;

export type IpcErrorCode = (typeof IPC_ERROR_CODES)[number];

/** 后端 `IpcError` 序列化后的形态 */
export interface IpcErrorPayload {
  code: IpcErrorCode;
  message: string;
  details: unknown;
  i18nKey: string | null;
}

/**
 * 迁移到 `IpcError` 的命令被拒绝时抛出的错误。
 * `toString()` 返回原始文案，`String(error)` 与旧的字符串错误显示一致。
 */
export class IpcError extends Error {
  readonly code: IpcErrorCode;
  readonly details: unknown;
  readonly i18nKey: string | null;

  constructor(payload: IpcErrorPayload) {
    super(payload.message);
    this.name = "IpcError";
    this.code = payload.code;
    this.details = payload.details ?? null;
    this.i18nKey = payload.i18nKey ?? null;
  }

  override toString(): string {
    return this.message;
  }
}

export function isIpcErrorPayload(value: unknown): value is IpcErrorPayload {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

/** 将 invoke 的拒绝值规范化：结构化错误转为 `IpcError`，其余原样返回 */
export function toIpcError(error: unknown): unknown {
  if (error instanceof IpcError) return error;
  if (isIpcErrorPayload(error)) return new IpcError(error);
  return error;
}

/** 读取错误码；非结构化错误（尚未迁移的字符串错误）返回 null */
export function getIpcErrorCode(error: unknown): IpcErrorCode | null {
  const normalized = toIpcError(error);
  return normalized instanceof IpcError ? normalized.code : null;
}

/** 与 `@tauri-apps/api/core` 的 invoke 相同，但拒绝值会经过 `toIpcError` */
export async function invoke<T>(
  cmd: string,
  ...rest: [args?: InvokeArgs, options?: InvokeOptions]
): Promise<T> {
  try {
    // 原样转发参数个数，避免给调用方补上多余的 undefined
    return await tauriInvoke<T>(cmd, ...rest);
  } catch (error) {
    throw toIpcError(error);
  }
}
