import type { TFunction } from "i18next";
import { toast } from "sonner";
import {
  classifyFetchModelsError,
  type FetchModelsErrorKind,
} from "@/lib/errors/fetchModelsError";

const MESSAGE_KEYS: Record<FetchModelsErrorKind, string> = {
  authFailed: "providerForm.fetchModelsAuthFailed",
  endpointNotFound: "providerForm.fetchModelsEndpointNotFound",
  timeout: "providerForm.fetchModelsTimeout",
  notSupported: "providerForm.fetchModelsNotSupported",
  unknown: "providerForm.fetchModelsFailed",
};

/**
 * Toast for a failed "fetch models" action. Lives in the UI layer; the API
 * module only fetches (FE-ARCH-29) and the classification is shared
 * (`classifyFetchModelsError`, FE-ARCH-11).
 */
export function showFetchModelsError(
  err: unknown,
  t: TFunction,
  opts?: { hasApiKey: boolean; hasBaseUrl: boolean },
): void {
  // Pre-flight: required fields are missing.
  if (opts && !opts.hasBaseUrl && !opts.hasApiKey) {
    toast.error(t("providerForm.fetchModelsNeedConfig"));
    return;
  }
  if (opts && !opts.hasApiKey) {
    toast.error(t("providerForm.fetchModelsNeedApiKey"));
    return;
  }
  if (opts && !opts.hasBaseUrl) {
    toast.error(t("providerForm.fetchModelsNeedEndpoint"));
    return;
  }
  toast.error(t(MESSAGE_KEYS[classifyFetchModelsError(err)]));
}
