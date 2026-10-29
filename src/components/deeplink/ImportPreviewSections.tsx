import { useTranslation } from "react-i18next";
import type { DeepLinkImportPreview } from "@/lib/api/deeplink";
import { maskFieldValue } from "@/lib/deeplink/mask";
import { Checkbox } from "@/components/ui/checkbox";

const RISK_BOX =
  "rounded-lg border border-red-300 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-3 space-y-2";

/** Full usage-script source plus the hosts it talks to (always risky). */
export function UsageScriptPreview({
  code,
  targetHosts,
}: {
  code: string;
  targetHosts?: string[];
}) {
  const { t } = useTranslation();
  return (
    <div data-testid="deeplink-usage-script" role="alert" className={RISK_BOX}>
      <div className="text-xs font-semibold text-red-700 dark:text-red-300">
        {t("deeplink.preview.usageScriptCode")}
      </div>
      <pre
        data-testid="deeplink-usage-script-code"
        className="text-xs font-mono bg-background p-2 rounded max-h-64 overflow-auto whitespace-pre-wrap break-all"
      >
        {code}
      </pre>
      {targetHosts && (
        <div className="text-xs space-y-1">
          <div className="font-medium text-red-700 dark:text-red-300">
            {t("deeplink.preview.usageScriptHosts")}
          </div>
          {targetHosts.length > 0 ? (
            <ul
              data-testid="deeplink-usage-script-hosts"
              className="space-y-0.5"
            >
              {targetHosts.map((host) => (
                <li key={host} className="font-mono break-all">
                  {host}
                </li>
              ))}
            </ul>
          ) : (
            <div className="text-muted-foreground">
              {t("deeplink.preview.usageScriptNoHosts")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Live config files the import writes now, or on the first switch. */
export function LiveFilesPreview({
  preview,
}: {
  preview: DeepLinkImportPreview;
}) {
  const { t } = useTranslation();
  if (preview.liveFiles.length === 0) return null;
  return (
    <div data-testid="deeplink-live-files" className="text-xs space-y-1">
      <div className="font-medium text-muted-foreground">
        {preview.writesLiveOnImport
          ? t("deeplink.preview.liveFilesOnImport")
          : t("deeplink.preview.liveFilesOnSwitch")}
      </div>
      <ul className="space-y-0.5">
        {preview.liveFiles.map((file) => (
          <li key={file} className="font-mono break-all">
            {file}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Field diff against an existing provider with the same name. */
export function ExistingProviderDiff({
  existing,
}: {
  existing: NonNullable<DeepLinkImportPreview["existing"]>;
}) {
  const { t } = useTranslation();
  const show = (field: string, value?: string) =>
    value === undefined ? "—" : maskFieldValue(field, value);

  return (
    <div
      data-testid="deeplink-existing-diff"
      className="rounded-lg border border-yellow-300 dark:border-yellow-800 bg-yellow-50 dark:bg-yellow-900/20 p-3 space-y-2 text-xs"
    >
      <div className="font-semibold text-yellow-800 dark:text-yellow-200">
        {t("deeplink.preview.existingTitle", { name: existing.name })}
      </div>
      {existing.isCurrent && (
        <div className="text-yellow-800 dark:text-yellow-200">
          {t("deeplink.preview.existingIsCurrent")}
        </div>
      )}
      {existing.changes.length === 0 ? (
        <div>{t("deeplink.preview.existingNoChanges")}</div>
      ) : (
        <>
          <div className="text-yellow-800 dark:text-yellow-200">
            {t("deeplink.preview.existingDescription")}
          </div>
          <table className="w-full table-fixed border-collapse">
            <thead>
              <tr className="text-left text-muted-foreground">
                <th className="w-1/3 pr-2 font-medium">
                  {t("deeplink.preview.diffField")}
                </th>
                <th className="w-1/3 pr-2 font-medium">
                  {t("deeplink.preview.diffBefore")}
                </th>
                <th className="w-1/3 font-medium">
                  {t("deeplink.preview.diffAfter")}
                </th>
              </tr>
            </thead>
            <tbody>
              {existing.changes.map((change) => (
                <tr key={change.field} className="align-top border-t">
                  <td className="pr-2 py-1 font-mono break-all">
                    {change.field}
                  </td>
                  <td className="pr-2 py-1 font-mono break-all whitespace-pre-wrap text-muted-foreground line-through decoration-red-400/60">
                    {show(change.field, change.before)}
                  </td>
                  <td className="py-1 font-mono break-all whitespace-pre-wrap">
                    {show(change.field, change.after)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}

/** Extra confirmation required before importing a link with risky items. */
export function RiskAcknowledgement({
  checked,
  onCheckedChange,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className={RISK_BOX}>
      <label className="flex items-start gap-2 text-sm font-medium text-red-700 dark:text-red-300 cursor-pointer">
        <Checkbox
          data-testid="deeplink-risk-ack"
          checked={checked}
          onCheckedChange={(value) => onCheckedChange(value === true)}
          className="mt-0.5"
        />
        <span>{t("deeplink.preview.riskAck")}</span>
      </label>
      {!checked && (
        <div className="text-xs text-red-700 dark:text-red-300">
          {t("deeplink.preview.riskAckHint")}
        </div>
      )}
    </div>
  );
}
