/**
 * Realistic Tauri invoke rejections for MSW handlers (TEST-26).
 *
 * A real `invoke()` rejects with the command's serialized error value itself,
 * not with an `Error`:
 * - `Result<_, String>` commands reject with a plain string;
 * - `Result<_, AppError>` commands reject with `{ code, message, params? }`
 *   (the exact shape is snapshotted from Rust into
 *   tests/contracts/app_error.json by src-tauri/tests/contracts.rs).
 *
 * `tauriError(...)` returns an MSW response that the invoke mock in
 * tests/msw/tauriMocks.ts turns into exactly that rejection value.
 */
import { HttpResponse } from "msw";
import type { AppErrorPayload } from "@/lib/bindings";
import appErrorContract from "../contracts/app_error.json";

/** Header marking a response as a serialized command error. */
export const TAURI_ERROR_HEADER = "x-tauri-invoke-error";

export type AppErrorSample = keyof typeof appErrorContract;

/** The Rust-produced payload for one of the contract samples. */
export function appErrorSample(name: AppErrorSample): AppErrorPayload {
  return structuredClone(appErrorContract[name]) as AppErrorPayload;
}

/** Build the payload `AppError::localized(key, zh, en)` serializes to. */
export function localizedAppError(
  key: string,
  en: string,
  zh: string = en,
  extra: Record<string, string> = {},
): AppErrorPayload {
  return {
    code: key,
    message: `${zh} (${en})`,
    params: { ...extra, en, zh },
  };
}

/**
 * MSW response for a rejected command.
 * - a string: the rejection of a `Result<_, String>` command;
 * - `{ key, en?, zh?, params? }`: an `AppError::Localized` rejection;
 * - an `AppErrorPayload`: passed through as-is.
 */
export function tauriError(
  error:
    | string
    | AppErrorPayload
    | {
        key: string;
        en?: string;
        zh?: string;
        params?: Record<string, string>;
      },
) {
  let body: unknown = error;
  if (typeof error === "object" && "key" in error) {
    body = localizedAppError(
      error.key,
      error.en ?? error.key,
      error.zh,
      error.params,
    );
  }
  return HttpResponse.json(body as never, {
    status: 500,
    headers: { [TAURI_ERROR_HEADER]: "1" },
  });
}
