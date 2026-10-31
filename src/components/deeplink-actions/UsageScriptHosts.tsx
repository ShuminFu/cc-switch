import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";

/** Every http(s) host a usage script literally references (PRODUCT-36). */
export function extractScriptHosts(script: string): string[] {
  const hosts: string[] = [];
  for (const match of script.matchAll(/https?:\/\/[^\s"'`<>\\)]+/gi)) {
    try {
      const host = new URL(match[0]).host.toLowerCase();
      if (host && !hosts.includes(host)) hosts.push(host);
    } catch {
      // not a URL after all
    }
  }
  return hosts;
}

/** Loopback, link-local and RFC 1918 targets (a script could probe the LAN). */
export function isPrivateHost(host: string): boolean {
  const h = host
    .replace(/:\d+$/, "")
    .replace(/^\[|\]$/g, "")
    .toLowerCase();
  if (h === "localhost" || h.endsWith(".localhost") || h.endsWith(".local"))
    return true;
  if (h === "::1" || h.startsWith("fe80:") || /^f[cd][0-9a-f]{2}:/.test(h))
    return true;
  const m = h.match(/^(\d+)\.(\d+)\.(\d+)\.(\d+)$/);
  if (!m) return false;
  const [a, b] = [Number(m[1]), Number(m[2])];
  return (
    a === 127 ||
    a === 10 ||
    a === 0 ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168)
  );
}

/**
 * Names the hosts a deep-linked usage script talks to, next to its opt-in
 * checkbox, so enabling it is an informed decision about where the API key
 * (`{{apiKey}}`) can go. Hosts built at runtime cannot be listed; the notice
 * says so.
 */
export function UsageScriptHosts({ script }: { script: string | null }) {
  const { t } = useTranslation();
  if (!script) return null;
  const hosts = extractScriptHosts(script);
  const sendsKey = /\{\{\s*apiKey\s*\}\}|apiKey/.test(script);
  const privateHosts = hosts.filter(isPrivateHost);

  return (
    <div
      className="flex items-start gap-2 rounded border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-800 dark:text-amber-200"
      data-testid="deeplink-usage-script-hosts"
    >
      <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
      <div className="space-y-1">
        {hosts.length > 0 ? (
          <p>
            {sendsKey
              ? t("deeplink.usageScriptSendsKeyTo", {
                  defaultValue:
                    "If enabled, this script can send your API key to: {{hosts}}",
                  hosts: hosts.join(", "),
                })
              : t("deeplink.usageScriptContacts", {
                  defaultValue: "If enabled, this script contacts: {{hosts}}",
                  hosts: hosts.join(", "),
                })}
          </p>
        ) : (
          <p>
            {t("deeplink.usageScriptNoHosts", {
              defaultValue:
                "This script builds its request URL at runtime; review the source to see where it sends data.",
            })}
          </p>
        )}
        {privateHosts.length > 0 && (
          <p className="font-medium">
            {t("deeplink.usageScriptPrivateHosts", {
              defaultValue:
                "It also targets local or private-network addresses: {{hosts}}",
              hosts: privateHosts.join(", "),
            })}
          </p>
        )}
      </div>
    </div>
  );
}
