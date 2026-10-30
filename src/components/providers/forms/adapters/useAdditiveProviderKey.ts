import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { isValidProviderKey } from "./providerKey";

/** i18n namespace of the provider-key strings of an additive app. */
export type ProviderKeyNamespace = "opencode" | "openclaw" | "hermes.form";

interface QueryState<T> {
  data?: T;
  isLoading: boolean;
  isError: boolean;
}

interface UseAdditiveProviderKeyParams {
  namespace: ProviderKeyNamespace;
  providerKey: string;
  providerId?: string;
  isEditMode: boolean;
  /** Providers already stored for this app (keys of the providers map). */
  storedProviders: QueryState<Record<string, unknown> | undefined>;
  /** Provider ids present in the app's live config file. */
  liveProviderIds: QueryState<string[]>;
}

export interface AdditiveProviderKeyState {
  namespace: ProviderKeyNamespace;
  /** Key exists in the live config, so renaming it is not allowed. */
  isLocked: boolean;
  /** Key status is unknown yet; the field is disabled and submit refused. */
  isStatusLoading: boolean;
  /** Key status could not be determined (a query failed, add mode only). */
  isStatusUnavailable: boolean;
  isDuplicate: boolean;
  isFormatInvalid: boolean;
  /** Hard submit error for the current key, or `null` when it is fine. */
  getSubmitError: () => string | null;
}

/**
 * Owns everything about the provider key of an additive-config app: the
 * existing/live key lookups, locked state and validation (FE-PROV-52).
 *
 * In add mode the duplicate check is only trustworthy once both lookups
 * succeeded: while either query is pending or failed, submit is refused
 * instead of letting `add_provider` silently overwrite an existing provider
 * with the same key (FE-PROV-44).
 */
export function useAdditiveProviderKey({
  namespace,
  providerKey,
  providerId,
  isEditMode,
  storedProviders,
  liveProviderIds,
}: UseAdditiveProviderKeyParams): AdditiveProviderKeyState {
  const { t } = useTranslation();

  const liveIds = liveProviderIds.data;
  const existingKeys = useMemo(() => {
    const keys = new Set<string>([
      ...Object.keys(storedProviders.data ?? {}),
      ...(liveIds ?? []),
    ]);
    if (providerId) keys.delete(providerId);
    return keys;
  }, [storedProviders.data, liveIds, providerId]);

  const isLocked =
    isEditMode && !!providerId && (liveIds ?? []).includes(providerId);
  const isStatusUnavailable =
    !isEditMode && (storedProviders.isError || liveProviderIds.isError);
  const isStatusLoading =
    liveProviderIds.isLoading ||
    (!isEditMode && storedProviders.isLoading) ||
    isStatusUnavailable;

  const trimmed = providerKey.trim();
  const isDuplicate = !isLocked && existingKeys.has(providerKey);
  const isFormatInvalid = trimmed !== "" && !isValidProviderKey(providerKey);

  const getSubmitError = useCallback((): string | null => {
    // providerKey is the row id in the app config: empty / malformed keys are
    // integrity errors, not "save anyway" issues.
    if (!trimmed) return t(`${namespace}.providerKeyRequired`);
    if (!isValidProviderKey(providerKey)) {
      return t(`${namespace}.providerKeyInvalid`);
    }
    if (isStatusUnavailable) {
      return t("providerForm.providerKeyStatusUnavailable", {
        defaultValue:
          "Could not check whether this provider key is already in use. Reload and try again.",
      });
    }
    if (isStatusLoading) {
      return t("providerForm.providerKeyStatusLoading", {
        defaultValue:
          "Still loading provider key status, please try again in a moment",
      });
    }
    if (isDuplicate) return t(`${namespace}.providerKeyDuplicate`);
    return null;
  }, [
    isDuplicate,
    isStatusLoading,
    isStatusUnavailable,
    namespace,
    providerKey,
    t,
    trimmed,
  ]);

  return {
    namespace,
    isLocked,
    isStatusLoading,
    isStatusUnavailable,
    isDuplicate,
    isFormatInvalid,
    getSubmitError,
  };
}
