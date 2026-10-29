import type { TFunction } from "i18next";
import { getIpcErrorCode } from "@/lib/api/ipc";

/**
 * Extract a human-readable message from any rejection value
 * (IpcError, Error, string, or a `{ message | error | detail | payload }` object).
 * @param error the rejection value
 * @returns the message, or an empty string when none can be found
 */
export const extractErrorMessage = (error: unknown): string => {
  if (!error) return "";
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === "object") {
    const errObject = error as Record<string, unknown>;

    const candidate = errObject.message ?? errObject.error ?? errObject.detail;
    if (typeof candidate === "string" && candidate.trim()) {
      return candidate;
    }

    const payload = errObject.payload;
    if (typeof payload === "string" && payload.trim()) {
      return payload;
    }
    if (payload && typeof payload === "object") {
      const payloadObj = payload as Record<string, unknown>;
      const payloadCandidate =
        payloadObj.message ?? payloadObj.error ?? payloadObj.detail;
      if (typeof payloadCandidate === "string" && payloadCandidate.trim()) {
        return payloadCandidate;
      }
    }
  }

  return "";
};

/**
 * Map an MCP command rejection to an i18n summary by its `IpcError.code`;
 * the backend message text is never parsed.
 * Returns an empty string for unmapped codes and for legacy (non-IpcError)
 * rejections, so callers fall back to `extractErrorMessage` or a default text.
 */
export const translateMcpBackendError = (
  error: unknown,
  t: TFunction,
): string => {
  switch (getIpcErrorCode(error)) {
    case "MCP_VALIDATION":
      return t("mcp.error.validationFailed");
    case "JSON":
      return t("mcp.error.jsonInvalid");
    case "TOML":
      return t("mcp.error.tomlInvalid");
    default:
      return "";
  }
};
