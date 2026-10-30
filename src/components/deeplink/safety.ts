/**
 * Helpers used by the deep-link confirmation dialogs to show untrusted
 * payloads faithfully (full command lines, endpoint hosts) while masking
 * secret-looking values.
 */

const SECRET_KEY_PATTERN =
  /(TOKEN|KEY|SECRET|PASSWORD|PASSWD|PWD|AUTH|CREDENTIAL|COOKIE|SESSION|PRIVATE)/i;

/** Whether an env/header name looks like it holds a secret. */
export function isSecretKey(key: string): boolean {
  return SECRET_KEY_PATTERN.test(key);
}

/** Mask a value: keep a short prefix so users can recognise it. */
export function maskSecret(value: string): string {
  if (value.length <= 4) return "****";
  const prefix = value.length > 12 ? value.substring(0, 4) : "";
  return `${prefix}${"*".repeat(12)}`;
}

/** Mask `value` when `key` looks secret, otherwise return it unchanged. */
export function maskIfSecret(key: string, value: unknown): string {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  return isSecretKey(key) ? maskSecret(str ?? "") : (str ?? "");
}

/** Extract the host (with port) of a URL, or return the trimmed input. */
export function getEndpointHost(endpoint: string): string {
  const trimmed = endpoint.trim();
  try {
    return new URL(trimmed).host || trimmed;
  } catch {
    return trimmed;
  }
}

function quoteArg(arg: string): string {
  if (arg === "") return '""';
  return /[\s"'`$&|<>^;()*?!\\]/.test(arg) ? JSON.stringify(arg) : arg;
}

/** Render `command` + `args` as a single, unambiguous command line. */
export function formatCommandLine(command: unknown, args: unknown): string {
  const parts = [String(command ?? "")];
  if (Array.isArray(args)) {
    for (const arg of args) {
      parts.push(quoteArg(typeof arg === "string" ? arg : JSON.stringify(arg)));
    }
  }
  return parts.join(" ");
}

const SHELL_LIKE_COMMANDS = new Set([
  "sh",
  "bash",
  "zsh",
  "fish",
  "dash",
  "ksh",
  "cmd",
  "powershell",
  "pwsh",
  "curl",
  "wget",
  "osascript",
]);

/**
 * Heuristic: does this stdio spec run a shell/downloader/interpreter, or
 * contain shell metacharacters that `cmd /c` wrapping on Windows would
 * interpret? Used only to show an extra warning.
 */
export function isRiskyCommand(command: unknown, args: unknown): boolean {
  if (typeof command !== "string") return false;
  const base = command
    .split(/[\\/]/)
    .pop()!
    .toLowerCase()
    .replace(/\.(exe|cmd|bat|ps1)$/, "");
  if (SHELL_LIKE_COMMANDS.has(base)) return true;
  if (!Array.isArray(args)) return false;
  return args.some(
    (arg) =>
      typeof arg === "string" &&
      (/[&|<>^;`]|\$\(/.test(arg) || /\b(curl|wget|iex|eval)\b/i.test(arg)),
  );
}

/** Decode Base64 (standard or URL-safe, padding optional) as UTF-8. */
export function decodeBase64Lenient(input: string): string | null {
  try {
    let normalized = input.trim().replace(/ /g, "+");
    normalized = normalized.replace(/-/g, "+").replace(/_/g, "/");
    while (normalized.length % 4 !== 0) normalized += "=";
    const bin = atob(normalized);
    const bytes = Uint8Array.from(bin, (c) => c.codePointAt(0) || 0);
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}
