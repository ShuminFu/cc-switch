/**
 * Masking for secrets shown in the deeplink import dialog.
 *
 * At most the first 4 characters are revealed, followed by a fixed run of
 * asterisks so the secret's length is not disclosed. Values of 8 characters
 * or fewer are shown as "****" only, so a short secret is never half exposed.
 */
const VISIBLE_CHARS = 4;
const MIN_LENGTH_FOR_PREFIX = 8;
const MASK_SUFFIX = "*".repeat(20);
const FULL_MASK = "****";

const SENSITIVE_KEY_PARTS = ["TOKEN", "KEY", "SECRET", "PASSWORD"];

export function maskSecret(value: string | null | undefined): string {
  if (!value || value.length <= MIN_LENGTH_FOR_PREFIX) {
    return FULL_MASK;
  }
  return `${value.substring(0, VISIBLE_CHARS)}${MASK_SUFFIX}`;
}

export function isSensitiveKey(key: string): boolean {
  const upper = key.toUpperCase();
  return SENSITIVE_KEY_PARTS.some((part) => upper.includes(part));
}

/** Masks `value` when `key` names a secret; other values are returned as is. */
export function maskValue(key: string, value: string): string {
  return isSensitiveKey(key) ? maskSecret(value) : value;
}
