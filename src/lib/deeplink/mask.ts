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

const CONFIG_LINE = /^(\s*["']?([A-Za-z0-9_.-]+)["']?\s*[=:]\s*)(.+)$/;

/**
 * Masks secret values inside multi-line config text (TOML `key = value`,
 * YAML / JSON `key: value`). Lines whose key does not name a secret are kept.
 */
export function maskConfigText(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      const match = line.match(CONFIG_LINE);
      if (!match || !isSensitiveKey(match[2])) return line;
      const raw = match[3]
        .trim()
        .replace(/,$/, "")
        .replace(/^["']|["']$/g, "");
      return `${match[1]}"${maskSecret(raw)}"`;
    })
    .join("\n");
}

/**
 * Masks a preview diff value addressed by a dotted `field` path
 * (e.g. `settingsConfig.env.ANTHROPIC_AUTH_TOKEN`).
 */
export function maskFieldValue(field: string, value: string): string {
  const parts = field.split(".");
  const leaf = parts[parts.length - 1] ?? field;
  // Header values (Authorization, ANTHROPIC_CUSTOM_HEADERS, ...) carry
  // credentials under names the key check does not catch, so always mask them
  if (
    isSensitiveKey(leaf) ||
    parts.some((part) => /header|authorization|cookie/i.test(part))
  ) {
    return maskSecret(value);
  }
  return value.includes("\n") ? maskConfigText(value) : value;
}
