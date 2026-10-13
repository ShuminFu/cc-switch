/**
 * Static summary of where a (deep-link supplied) usage script sends requests.
 *
 * Usage scripts run periodically with the provider's API key substituted into
 * `{{apiKey}}`. Before importing one from a link, the user should see which
 * hosts it will contact — a script that talks to a host other than the
 * provider endpoint could leak the key.
 */
export interface UsageScriptHostSummary {
  /** Host the script's `{{baseUrl}}` resolves to (usage base URL or endpoint). */
  baseHost: string | null;
  /** Hosts of absolute http(s) URLs written literally in the script. */
  scriptHosts: string[];
  /** Entries of `scriptHosts` that differ from `baseHost`. */
  foreignHosts: string[];
  /** Whether the script uses the `{{baseUrl}}` template variable. */
  usesBaseUrl: boolean;
}

const URL_LITERAL = /https?:\/\/[^\s'"`<>()\\]+/gi;

function hostOf(raw: string | undefined | null): string | null {
  if (!raw) return null;
  try {
    const url = new URL(raw.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    return url.host.toLowerCase();
  } catch {
    return null;
  }
}

export function summarizeUsageScriptHosts(
  scriptCode: string,
  baseUrl: string | undefined | null,
): UsageScriptHostSummary {
  const baseHost = hostOf(baseUrl);
  const hosts = new Set<string>();
  for (const match of scriptCode.match(URL_LITERAL) ?? []) {
    // Only the scheme + authority matter; template variables may appear in the
    // path/query (e.g. ?key={{apiKey}}). Skip templated hosts (https://{{host}}).
    const origin = match.match(/^https?:\/\/[^/?#]+/i)?.[0];
    if (!origin || origin.includes("{")) continue;
    const host = hostOf(origin);
    if (host) hosts.add(host);
  }
  const scriptHosts = [...hosts].sort();
  const foreignHosts = scriptHosts.filter((h) => h !== baseHost);
  return {
    baseHost,
    scriptHosts,
    foreignHosts,
    usesBaseUrl: scriptCode.includes("{{baseUrl}}"),
  };
}
