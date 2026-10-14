import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { summarizeUsageScriptHosts } from "@/lib/utils/usageScriptHosts";

/**
 * Shows which hosts a deep-link supplied usage script will contact, and warns
 * when it contains hosts other than the provider endpoint — the script runs
 * periodically with the provider's API key substituted in.
 */
export function UsageScriptHostNotice({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  const { t } = useTranslation();

  const summary = useMemo(() => {
    if (!request.usageScript) return null;
    // Fall back to the raw parameter if the backend could not decode it.
    const code = request.decoded?.usageScript ?? request.usageScript;
    return summarizeUsageScriptHosts(
      code,
      request.usageBaseUrl || request.endpoint?.split(",")[0],
    );
  }, [
    request.usageScript,
    request.decoded?.usageScript,
    request.usageBaseUrl,
    request.endpoint,
  ]);

  if (!summary) return null;

  const hosts = [
    ...new Set(
      [summary.usesBaseUrl ? summary.baseHost : null, ...summary.scriptHosts]
        .filter((h): h is string => !!h)
        .sort(),
    ),
  ];

  return (
    <>
      {hosts.length > 0 && (
        <div className="grid grid-cols-3 items-start gap-4">
          <div className="font-medium text-sm text-muted-foreground">
            {t("deeplink.usageScriptHosts")}
          </div>
          <div className="col-span-2 text-sm font-mono break-all">
            {hosts.join(", ")}
          </div>
        </div>
      )}
      {summary.foreignHosts.length > 0 ? (
        <div
          role="alert"
          className="rounded-lg bg-red-50 dark:bg-red-900/20 p-3 text-sm font-medium text-red-800 dark:text-red-200"
        >
          {t("deeplink.usageScriptForeignHostsWarning", {
            hosts: summary.foreignHosts.join(", "),
          })}
        </div>
      ) : (
        <div className="text-sm text-yellow-700 dark:text-yellow-400">
          {t("deeplink.usageScriptKeyNotice")}
        </div>
      )}
    </>
  );
}
