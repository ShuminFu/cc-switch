import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { toBackendError } from "@/lib/errors/appError";

/**
 * `invoke` for the hand-written wrappers in `src/lib/api`: identical to
 * `@tauri-apps/api/core`'s, except that a structured `AppError` rejection
 * (`{ code, message, params }`) is rethrown as a localized `BackendError`.
 * Plain string rejections from commands not yet converted pass through as-is.
 */
export async function invoke<T>(
  ...args: Parameters<typeof tauriInvoke>
): Promise<T> {
  try {
    return await tauriInvoke<T>(...args);
  } catch (error) {
    throw toBackendError(error);
  }
}

type BindingResult<T, E> =
  | { status: "ok"; data: T }
  | { status: "error"; error: E };

/**
 * Unwrap a result from the generated `commands.*` bindings (`src/lib/bindings.ts`):
 * return the data, or throw the error exactly as a raw `invoke` would have
 * (structured errors become a `BackendError`).
 */
export function unwrap<T, E>(result: BindingResult<T, E>): T {
  if (result.status === "ok") return result.data;
  throw toBackendError(result.error);
}

/** Await a generated binding call and unwrap it. */
export async function call<T, E>(
  pending: Promise<BindingResult<T, E>>,
): Promise<T> {
  return unwrap(await pending);
}
