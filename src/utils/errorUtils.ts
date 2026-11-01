import {
  getErrorCode,
  isAppErrorPayload,
  localizeAppError,
} from "@/lib/errors/appError";

/**
 * Extract a displayable message from any error value.
 *
 * Structured backend errors (`{ code, message, params }`, see
 * `src/lib/errors/appError.ts`) are rendered in the current UI language.
 * @param error the caught value
 * @returns the message, or "" when none can be found
 */
export const extractErrorMessage = (error: unknown): string => {
  if (!error) return "";
  if (typeof error === "string") {
    return error;
  }
  if (isAppErrorPayload(error)) {
    return localizeAppError(error);
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
    if (isAppErrorPayload(payload)) {
      return localizeAppError(payload);
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
 * Backend error codes the MCP form renders with a form-specific message. The
 * codes are the i18n keys `AppError::Localized` carries from
 * `src-tauri/src/mcp/validation.rs` and `src-tauri/src/claude_mcp.rs`, plus
 * the generic `AppError` variants that only occur for MCP config files.
 */
const MCP_ERROR_CODES: Record<string, string> = {
  "mcp.error.idRequired": "mcp.error.idRequired",
  "mcp.error.jsonInvalid": "mcp.error.jsonInvalid",
  "mcp.error.commandRequired": "mcp.error.commandRequired",
  "mcp.wizard.urlRequired": "mcp.wizard.urlRequired",
  "mcp.error.tomlInvalid": "mcp.error.tomlInvalid",
  "errors.toml": "mcp.error.tomlInvalid",
};

/**
 * Map a known MCP backend error to its i18n message by error code (CMD-12).
 *
 * Accepts the caught error (preferred) or an already-extracted message string.
 * Returns an empty string when the error carries no known code, so the caller
 * falls back to the backend's own (localized) detail or a default message.
 */
export const translateMcpBackendError = (
  error: unknown,
  t: (key: string, opts?: Record<string, unknown>) => string,
): string => {
  const code = getErrorCode(error);
  if (!code) return "";
  const key = MCP_ERROR_CODES[code];
  return key ? t(key) : "";
};
