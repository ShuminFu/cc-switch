import { getErrorCode, isAppErrorPayload } from "@/lib/errors/appError";

/**
 * Classified failure of `fetch_models_for_config` (FE-ARCH-11). One place
 * decides what a model-fetch error means; the UI maps the kind to a message.
 */
export type FetchModelsErrorKind =
  | "authFailed"
  | "endpointNotFound"
  | "timeout"
  | "notSupported"
  | "unknown";

function httpStatusOf(error: unknown): number | undefined {
  // Structured `AppError::HttpStatus` carries the status in params.
  if (getErrorCode(error) === "errors.httpStatus") {
    const payload = isAppErrorPayload(error)
      ? error
      : (error as { payload?: unknown }).payload;
    const status = isAppErrorPayload(payload)
      ? Number(payload.params?.status)
      : NaN;
    if (Number.isFinite(status)) return status;
  }
  const match = /\bHTTP\s+(\d{3})\b/.exec(String(error ?? ""));
  return match ? Number(match[1]) : undefined;
}

export function classifyFetchModelsError(error: unknown): FetchModelsErrorKind {
  const status = httpStatusOf(error);
  if (status === 401 || status === 403) return "authFailed";
  if (status === 404 || status === 405) return "endpointNotFound";

  const text = String(error ?? "");
  // Every candidate URL answered 404/405 (backend `All candidates failed`).
  if (/all candidates failed/i.test(text)) return "endpointNotFound";
  if (/\btime(?:d)?\s?out\b|timeout/i.test(text)) return "timeout";
  if (/failed to parse/i.test(text)) return "notSupported";
  return "unknown";
}
