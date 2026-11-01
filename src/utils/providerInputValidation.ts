/**
 * Shared endpoint-URL and API-key hygiene for every provider form
 * (FE-PROV-21 / FE-PROV-36). Before this only Hermes validated its base URL
 * client-side; typos like `htp://x`, or keys pasted with quotes, a `Bearer `
 * prefix or a trailing newline, surfaced only at switch/request time.
 */

export type EndpointUrlError = "empty" | "invalid" | "scheme";

const TEMPLATE_TOKEN_RE = /\$\{[^}]+\}/g;

/**
 * Validate an upstream endpoint. Template tokens such as `${AWS_REGION}`
 * (Bedrock / KAT-Coder presets) are accepted.
 */
export function validateEndpointUrl(
  raw: string,
  { allowEmpty = false }: { allowEmpty?: boolean } = {},
): EndpointUrlError | null {
  const trimmed = raw.trim();
  if (!trimmed) return allowEmpty ? null : "empty";
  const candidate = trimmed.replace(TEMPLATE_TOKEN_RE, "placeholder");
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return "invalid";
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return "scheme";
  if (!url.hostname) return "invalid";
  return null;
}

/** i18n key + English default for an endpoint error. */
export const ENDPOINT_URL_ERROR_MESSAGES: Record<
  EndpointUrlError,
  { key: string; defaultValue: string }
> = {
  empty: {
    key: "providerForm.endpointRequiredInline",
    defaultValue: "Enter the API endpoint",
  },
  invalid: {
    key: "providerForm.endpointInvalid",
    defaultValue: "Enter a valid URL, e.g. https://api.example.com/v1",
  },
  scheme: {
    key: "providerForm.endpointScheme",
    defaultValue: "The endpoint must start with http:// or https://",
  },
};

const WRAPPING_QUOTES_RE = /^(["'`])(.*)\1$/s;

/**
 * Clean up a pasted API key: trim whitespace and line breaks, drop wrapping
 * quotes and a leading `Bearer ` prefix.
 */
export function normalizeApiKey(raw: string): string {
  let key = raw.replace(/[\r\n]+/g, "").trim();
  const quoted = key.match(WRAPPING_QUOTES_RE);
  if (quoted) key = quoted[2].trim();
  key = key.replace(/^bearer\s+/i, "");
  return key.trim();
}
