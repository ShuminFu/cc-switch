import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Checkbox } from "@/components/ui/checkbox";
import type { BundleItemChoice, DeepLinkImportRequestV2 } from "./api";
import { McpConfirmation } from "./McpConfirmation";
import { PromptConfirmation } from "./PromptConfirmation";
import { SkillConfirmation } from "./SkillConfirmation";
import { decodeBase64Lenient, getEndpointHost, maskSecret } from "./safety";

function ProviderSummary({
  item,
  choice,
  onChange,
}: {
  item: DeepLinkImportRequestV2;
  choice: BundleItemChoice;
  onChange: (next: BundleItemChoice) => void;
}) {
  const { t } = useTranslation();
  const switchId = useId();
  const usageId = useId();
  const host = getEndpointHost(item.endpoint?.split(",")[0]?.trim() ?? "");
  const script = item.usageScript
    ? decodeBase64Lenient(item.usageScript)
    : null;

  return (
    <div className="space-y-2 text-sm">
      <div className="rounded border border-blue-300 bg-blue-50 p-2 dark:border-blue-800 dark:bg-blue-900/20">
        <div className="text-xs text-muted-foreground">
          {t("deeplink.endpointHostLabel", {
            defaultValue: "Requests will be sent to",
          })}
        </div>
        <div className="font-mono font-semibold break-all">{host || "-"}</div>
      </div>
      <div className="grid grid-cols-3 gap-2">
        <span className="text-muted-foreground">{t("deeplink.app")}</span>
        <span className="col-span-2 capitalize">{item.app}</span>
        <span className="text-muted-foreground">
          {t("deeplink.providerName")}
        </span>
        <span className="col-span-2">{item.name}</span>
        {item.endpoint && (
          <>
            <span className="text-muted-foreground">
              {t("deeplink.endpoint")}
            </span>
            <span className="col-span-2 break-all font-mono text-xs">
              {item.endpoint}
            </span>
          </>
        )}
        {item.apiKey && (
          <>
            <span className="text-muted-foreground">
              {t("deeplink.apiKey")}
            </span>
            <span className="col-span-2 font-mono text-xs">
              {maskSecret(item.apiKey)}
            </span>
          </>
        )}
        {item.model && (
          <>
            <span className="text-muted-foreground">{t("deeplink.model")}</span>
            <span className="col-span-2 font-mono text-xs">{item.model}</span>
          </>
        )}
      </div>
      {item.usageScript && (
        <div className="space-y-1">
          <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded border bg-muted/50 p-2 text-xs font-mono">
            {script ??
              t("deeplink.usageScriptDecodeFailed", {
                defaultValue: "(script could not be decoded)",
              })}
          </pre>
          <div className="flex items-start gap-2">
            <Checkbox
              id={usageId}
              checked={choice.usageEnabled === true}
              disabled={script === null || !choice.accept}
              onCheckedChange={(v) =>
                onChange({ ...choice, usageEnabled: v === true })
              }
            />
            <label htmlFor={usageId} className="text-xs leading-tight">
              {t("deeplink.usageScriptEnableLabel", {
                defaultValue:
                  "Enable this usage script (it runs inside CC Switch on every usage query)",
              })}
            </label>
          </div>
        </div>
      )}
      <div className="flex items-start gap-2">
        <Checkbox
          id={switchId}
          checked={choice.enabled === true}
          disabled={!choice.accept}
          onCheckedChange={(v) => onChange({ ...choice, enabled: v === true })}
        />
        <label htmlFor={switchId} className="text-xs leading-tight">
          {t("deeplink.switchNowLabel", {
            defaultValue:
              "Switch to this provider now (route all {{app}} traffic to {{host}})",
            app: item.app,
            host,
          })}
        </label>
      </div>
    </div>
  );
}

const resourceLabel = (
  t: ReturnType<typeof useTranslation>["t"],
  resource: string,
) => {
  switch (resource) {
    case "provider":
      return t("deeplink.bundleProvider", { defaultValue: "Provider" });
    case "mcp":
      return t("deeplink.bundleMcp", { defaultValue: "MCP servers" });
    case "prompt":
      return t("deeplink.bundlePrompt", { defaultValue: "Prompt" });
    case "skill":
      return t("deeplink.bundleSkill", { defaultValue: "Skill repository" });
    default:
      return resource;
  }
};

/**
 * Every item of a multi-resource (v2) link with its full details and a
 * per-item accept toggle. Activation flags always start off.
 */
export function BundleConfirmation({
  items,
  choices,
  onChange,
}: {
  items: DeepLinkImportRequestV2[];
  choices: BundleItemChoice[];
  onChange: (choices: BundleItemChoice[]) => void;
}) {
  const { t } = useTranslation();
  const update = (next: BundleItemChoice) =>
    onChange(choices.map((c) => (c.index === next.index ? next : c)));

  return (
    <div className="space-y-3" data-testid="deeplink-bundle">
      <p className="text-sm text-muted-foreground">
        {t("deeplink.bundleDescription", {
          defaultValue:
            "This link contains {{count}} items. Review each one and untick anything you do not want to import.",
          count: items.length,
        })}
      </p>
      {items.map((item, index) => {
        const choice = choices.find((c) => c.index === index) ?? {
          index,
          accept: false,
        };
        const acceptId = `deeplink-bundle-accept-${index}`;
        return (
          <div
            key={index}
            className={`rounded-lg border p-3 ${choice.accept ? "" : "opacity-60"}`}
            data-testid={`deeplink-bundle-item-${index}`}
          >
            <div className="mb-2 flex items-center gap-2">
              <Checkbox
                id={acceptId}
                checked={choice.accept}
                onCheckedChange={(v) =>
                  update({
                    ...choice,
                    accept: v === true,
                    ...(v === true
                      ? {}
                      : { enabled: false, usageEnabled: false }),
                  })
                }
              />
              <label htmlFor={acceptId} className="text-sm font-semibold">
                {index + 1}. {resourceLabel(t, item.resource)}
                {item.name ? ` — ${item.name}` : ""}
              </label>
            </div>
            {item.resource === "provider" && (
              <ProviderSummary item={item} choice={choice} onChange={update} />
            )}
            {item.resource === "mcp" && (
              <McpConfirmation
                request={item}
                enableNow={choice.enabled === true}
                onEnableNowChange={(v) => update({ ...choice, enabled: v })}
              />
            )}
            {item.resource === "prompt" && (
              <PromptConfirmation
                request={item}
                enableNow={choice.enabled === true}
                onEnableNowChange={(v) => update({ ...choice, enabled: v })}
              />
            )}
            {item.resource === "skill" && <SkillConfirmation request={item} />}
          </div>
        );
      })}
    </div>
  );
}
