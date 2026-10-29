/**
 * Classification of env keys carried by a Claude deeplink config.
 *
 * Mirrors `classify_claude_env_key` in `src-tauri/src/deeplink/provider.rs`;
 * keep both lists in sync. Denied keys are dropped by the backend on import;
 * unlisted keys are imported, but the provider is not auto-switched to.
 */
export type ClaudeEnvKeyClass = "allowed" | "denied" | "unlisted";

const DENIED_ENV_KEYS = new Set([
  "NODE_OPTIONS",
  "NODE_PATH",
  "PATH",
  "BASH_ENV",
  "ENV",
  "PYTHONPATH",
  "PYTHONSTARTUP",
]);
const DENIED_ENV_PREFIXES = ["LD_", "DYLD_"];

const ALLOWED_ENV_KEYS = new Set([
  "API_TIMEOUT_MS",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
  "ALL_PROXY",
]);
const ALLOWED_ENV_PREFIXES = ["ANTHROPIC_", "CLAUDE_CODE_", "DISABLE_"];

export function classifyClaudeEnvKey(key: string): ClaudeEnvKeyClass {
  // ASCII-only upper-casing, like Rust's `to_ascii_uppercase`: `toUpperCase`
  // folds e.g. "ſ" to "S", which would hide a key the backend keeps as unlisted.
  const upper = key.trim().replace(/[a-z]/g, (c) => c.toUpperCase());
  if (
    DENIED_ENV_KEYS.has(upper) ||
    DENIED_ENV_PREFIXES.some((p) => upper.startsWith(p))
  ) {
    return "denied";
  }
  if (
    ALLOWED_ENV_KEYS.has(upper) ||
    ALLOWED_ENV_PREFIXES.some((p) => upper.startsWith(p))
  ) {
    return "allowed";
  }
  return "unlisted";
}
