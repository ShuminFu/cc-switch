/**
 * Mask secrets in text that is about to be shown or copied (error toasts,
 * previews) (FE-PROV-23). Keeps a short prefix and the last 4 characters so
 * the user can still recognise which key is meant: `sk-…abcd`.
 */

const mask = (secret: string): string => {
  if (secret.length <= 8) return "…";
  const prefixMatch = /^(sk-ant-|sk-|AIza|ghp_|gho_|xai-|Bearer\s+)/.exec(
    secret,
  );
  const prefix = prefixMatch ? prefixMatch[0] : "";
  return `${prefix}…${secret.slice(-4)}`;
};

const PATTERNS: RegExp[] = [
  // Well-known API key shapes.
  /\bsk-(?:ant-)?[A-Za-z0-9_-]{12,}/g,
  /\bAIza[0-9A-Za-z_-]{20,}/g,
  /\bgh[po]_[A-Za-z0-9]{20,}/g,
  /\bxai-[A-Za-z0-9]{20,}/g,
  // Authorization headers.
  /\bBearer\s+[A-Za-z0-9._~+/=-]{12,}/g,
];

// `"api_key": "value"`, `ANTHROPIC_AUTH_TOKEN = 'value'`, `token=value`.
const ASSIGNMENT =
  /((?:api[_-]?key|auth[_-]?token|access[_-]?token|secret|password|bearer[_-]?token|OPENAI_API_KEY|ANTHROPIC_(?:AUTH_TOKEN|API_KEY)|GEMINI_API_KEY)["']?\s*[:=]\s*["']?)([^"'\s,}&]{8,})/gi;

export function redactSecrets(text: string): string {
  if (!text) return text;
  let out = text.replace(
    ASSIGNMENT,
    (_m, lead: string, value: string) => lead + mask(value),
  );
  for (const pattern of PATTERNS) {
    out = out.replace(pattern, (secret) => mask(secret));
  }
  return out;
}
