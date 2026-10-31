import i18n from "i18next";
import type { AppErrorPayload } from "@/lib/bindings";

export type { AppErrorPayload };

/**
 * Structured backend errors (CMD-12, I18N-03).
 *
 * Rust `AppError` crosses IPC as `{ code, message, params? }`:
 * - `code` is a stable i18n key (`errors.config`, `sync.conflict.remote_changed`...)
 * - `message` is the legacy zh/en text, used as the fallback
 * - `params` holds interpolation values; `AppError::Localized` puts its `zh` and
 *   `en` fallbacks there.
 *
 * Commands that still return `Result<_, String>` reject with a plain string;
 * those pass through unchanged.
 */
export function isAppErrorPayload(value: unknown): value is AppErrorPayload {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.code === "string" &&
    v.code.length > 0 &&
    typeof v.message === "string" &&
    (v.params === undefined ||
      v.params === null ||
      typeof v.params === "object")
  );
}

type Translate = (key: string, options?: Record<string, unknown>) => string;

function isChineseLanguage(language: string | undefined): boolean {
  return !!language && language.toLowerCase().startsWith("zh");
}

/**
 * English templates for the generic `AppError` variants (`AppError::code()` in
 * `src-tauri/src/error.rs`). They are the `defaultValue` until the `errors.*`
 * keys are translated; `{{detail}}` etc. come from `params`.
 * `errors.message` is a free-form message, so its template is the detail alone.
 */
export const APP_ERROR_DEFAULTS: Record<string, string> = {
  "errors.config": "Configuration error: {{detail}}",
  "errors.invalidInput": "Invalid input: {{detail}}",
  "errors.io": "I/O error: {{path}}: {{detail}}",
  "errors.ioContext": "{{context}}: {{detail}}",
  "errors.json": "JSON parse error: {{path}}: {{detail}}",
  "errors.jsonSerialize": "JSON serialization failed: {{detail}}",
  "errors.toml": "TOML parse error: {{path}}: {{detail}}",
  "errors.lock": "Failed to acquire a lock: {{detail}}",
  "errors.mcpValidation": "MCP validation failed: {{detail}}",
  "errors.message": "{{detail}}",
  "errors.httpStatus": "HTTP {{status}}: {{body}}",
  "errors.database": "Database error: {{detail}}",
  "errors.omoConfigNotFound": "The OMO config file does not exist",
  "errors.allProvidersCircuitOpen":
    "All providers are circuit-broken; no provider is available",
  "errors.noProvidersConfigured": "No provider is configured",
};

type ErrorParams = Partial<Record<string, string>>;

/** Interpolation params without the `zh`/`en` fallbacks of a localized error. */
function interpolationParams(params: ErrorParams | null | undefined) {
  const out: ErrorParams = {};
  for (const [key, value] of Object.entries(params ?? {})) {
    if (key !== "zh" && key !== "en") out[key] = value;
  }
  return out;
}

function interpolate(template: string, params: ErrorParams) {
  return template.replace(
    /\{\{\s*(\w+)\s*\}\}/g,
    (match, name: string) => params[name] ?? match,
  );
}

/**
 * The untranslated fallback text: the zh/en half of a localized error that
 * matches the UI language, the English template of a generic error for
 * non-Chinese UIs, otherwise the backend message.
 */
export function fallbackMessage(
  payload: AppErrorPayload,
  language: string | undefined = i18n.language,
): string {
  const params = payload.params ?? {};
  if (isChineseLanguage(language)) {
    return params.zh || payload.message;
  }
  if (params.en) return params.en;
  const template = APP_ERROR_DEFAULTS[payload.code];
  if (template) {
    return interpolate(template, interpolationParams(params));
  }
  return payload.message;
}

/**
 * Render a structured error in the current UI language: `t(code, params)` when
 * the key is translated, otherwise the fallback text from the backend.
 */
export function localizeAppError(
  payload: AppErrorPayload,
  t?: Translate,
): string {
  const fallback = fallbackMessage(payload);
  if (payload.code === "errors.message") return fallback;
  const translate: Translate | undefined =
    t ??
    (i18n.isInitialized
      ? (key, options) => i18n.t(key, options) as unknown as string
      : undefined);
  if (!translate) return fallback;
  const out = translate(payload.code, {
    ...interpolationParams(payload.params),
    defaultValue: fallback,
  });
  return typeof out === "string" && out && out !== payload.code
    ? out
    : fallback;
}

/**
 * An `Error` carrying a backend error code. `message` is already localized, and
 * `toString()` returns it unchanged so legacy `String(error)` / `${error}`
 * call sites render the same text they rendered for string errors.
 */
export class BackendError extends Error {
  readonly code: string;
  readonly params?: ErrorParams;
  readonly payload: AppErrorPayload;

  constructor(payload: AppErrorPayload) {
    super(localizeAppError(payload));
    this.name = "BackendError";
    this.code = payload.code;
    this.params = payload.params ?? undefined;
    this.payload = payload;
  }

  override toString(): string {
    return this.message;
  }
}

/** Convert a raw IPC rejection into a `BackendError` when it is structured. */
export function toBackendError(raw: unknown): unknown {
  if (raw instanceof BackendError) return raw;
  if (isAppErrorPayload(raw)) return new BackendError(raw);
  return raw;
}

/** The backend error code of a rejection, if it carries one. */
export function getErrorCode(error: unknown): string | undefined {
  if (error instanceof BackendError) return error.code;
  if (isAppErrorPayload(error)) return error.code;
  if (error && typeof error === "object") {
    const payload = (error as { payload?: unknown }).payload;
    if (isAppErrorPayload(payload)) return payload.code;
  }
  return undefined;
}

/** True when `error` carries `code` (exact match). */
export function hasErrorCode(error: unknown, code: string): boolean {
  return getErrorCode(error) === code;
}
