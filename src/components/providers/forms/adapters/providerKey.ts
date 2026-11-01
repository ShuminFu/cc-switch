/**
 * Provider keys of additive-config apps (OpenCode / OpenClaw / Hermes) are the
 * primary key of the provider in the app's own config file. One definition of
 * the allowed shape, shared by the key field and submit validation
 * (FE-PROV-52).
 */
export const PROVIDER_KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

export const isValidProviderKey = (key: string): boolean =>
  PROVIDER_KEY_PATTERN.test(key);

/** Normalise typed input: lowercase, drop characters that can never match. */
export const sanitizeProviderKeyInput = (raw: string): string =>
  raw.toLowerCase().replace(/[^a-z0-9-]/g, "");
