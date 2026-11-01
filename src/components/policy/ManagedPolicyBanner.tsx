import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, ShieldAlert, ShieldCheck } from "lucide-react";
import { policyApi, type ManagedPolicyStatus } from "@/lib/api/policy";
import { queryKeys } from "@/lib/query/keys";

/**
 * "Managed by policy" banner (PRODUCT-24). Shows what an administrator's
 * policy.json enforces, or why a policy file present on disk is ignored.
 */
export function ManagedPolicyBanner() {
  const { data } = useQuery({
    queryKey: queryKeys.policy.status,
    queryFn: () => policyApi.getStatus(),
    staleTime: 60_000,
    retry: false,
  });
  if (!data || data.state === "none") return null;
  return <ManagedPolicyBannerView status={data} />;
}

export function ManagedPolicyBannerView({
  status,
}: {
  status: ManagedPolicyStatus;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  if (status.state !== "active" || !status.policy) {
    return (
      <div
        role="alert"
        className="mx-6 mb-2 flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200"
      >
        <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
        <div className="min-w-0">
          <div className="font-medium">
            {status.state === "signatureInvalid"
              ? t("policy.signatureInvalid", {
                  defaultValue:
                    "A managed policy file was ignored because its signature could not be verified.",
                })
              : t("policy.invalid", {
                  defaultValue:
                    "A managed policy file was ignored because it could not be read.",
                })}
          </div>
          {status.source && (
            <div className="truncate opacity-80">{status.source}</div>
          )}
          {status.error && <div className="opacity-80">{status.error}</div>}
        </div>
      </div>
    );
  }

  const { policy } = status;
  const name =
    policy.name?.trim() ||
    t("policy.defaultName", { defaultValue: "your organization" });
  const rows: Array<{ label: string; values: string[] }> = [
    {
      label: t("policy.allowedEndpoints", {
        defaultValue: "Allowed endpoints:",
      }),
      values: policy.endpoints.allow,
    },
    {
      label: t("policy.deniedEndpoints", { defaultValue: "Denied endpoints:" }),
      values: policy.endpoints.deny,
    },
    {
      label: t("policy.disabledFeatures", {
        defaultValue: "Disabled features:",
      }),
      values: policy.disabledFeatures,
    },
  ];

  return (
    <div
      role="status"
      className="mx-6 mb-2 rounded-lg border border-blue-500/30 bg-blue-500/10 px-3 py-2 text-xs text-blue-900 dark:text-blue-100"
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 text-left"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <ShieldCheck className="h-4 w-4 shrink-0" aria-hidden />
        <span className="flex-1 font-medium">
          {t("policy.managedBy", {
            defaultValue: "Managed by policy: {{name}}",
            name,
          })}
          {status.signed &&
            ` · ${t("policy.signed", { defaultValue: "signed" })}`}
        </span>
        {expanded ? (
          <ChevronUp className="h-4 w-4" aria-hidden />
        ) : (
          <ChevronDown className="h-4 w-4" aria-hidden />
        )}
      </button>
      {expanded && (
        <div className="mt-2 space-y-1 pl-6">
          {policy.message && <p>{policy.message}</p>}
          {rows
            .filter((row) => row.values.length > 0)
            .map((row) => (
              <p key={row.label}>
                <span className="font-medium">{row.label}</span>{" "}
                {row.values.map((value, i) => (
                  <span key={value}>
                    {i > 0 && ", "}
                    <code>{value}</code>
                  </span>
                ))}
              </p>
            ))}
          {status.source && (
            <p className="truncate opacity-70">{status.source}</p>
          )}
        </div>
      )}
    </div>
  );
}
