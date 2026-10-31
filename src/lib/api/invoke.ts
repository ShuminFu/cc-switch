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

/** Rust `()` is exported as `null`; the wrappers expose it as `void`. */
type Unit<T> = [T] extends [null] ? void : T;

/**
 * Unwrap a result from the generated `commands.*` bindings (`src/lib/bindings.ts`):
 * return the data, or throw the error exactly as a raw `invoke` would have
 * (structured errors become a `BackendError`).
 */
export function unwrap<T, E>(result: BindingResult<T, E>): Unit<T> {
  if (result.status === "ok") return result.data as Unit<T>;
  throw toBackendError(result.error);
}

/** Await a generated binding call and unwrap it. */
export async function call<T, E>(
  pending: Promise<BindingResult<T, E>>,
): Promise<Unit<T>> {
  return unwrap(await pending);
}

/**
 * Like `call`, for commands whose Rust return type is still an untyped
 * `serde_json::Value` (`JsonValue` in the bindings): the wrapper's declared
 * return type documents the shape until the command gets a typed DTO.
 */
export async function callJson<T>(
  pending: Promise<BindingResult<unknown, unknown>>,
): Promise<T> {
  return unwrap(await pending) as T;
}
