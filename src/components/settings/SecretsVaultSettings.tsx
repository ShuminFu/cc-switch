import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  secretsVaultApi,
  type SecretsVaultStatus,
} from "@/lib/api/secretsVault";
import { extractErrorMessage } from "@/utils/errorUtils";

const QUERY_KEY = ["secretsVaultStatus"];

/**
 * Opt-in provider API-key vault (DB-02 / PRODUCT-01): keeps provider keys in
 * the system keychain and only placeholders in the database. Off by default.
 */
export function SecretsVaultSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: status } = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => secretsVaultApi.getStatus(),
  });

  const toggle = useMutation({
    mutationFn: (enabled: boolean) => secretsVaultApi.setEnabled(enabled),
    onSuccess: (next: SecretsVaultStatus) => {
      queryClient.setQueryData(QUERY_KEY, next);
      if (next.warning) {
        toast.warning(next.warning);
      } else {
        toast.success(
          next.enabled
            ? t("settings.secretsVault.enabled", {
                defaultValue:
                  "Provider API keys are now stored in the system keychain.",
              })
            : t("settings.secretsVault.disabled", {
                defaultValue:
                  "Provider API keys are stored in the database again.",
              }),
        );
      }
    },
    onError: (error) => toast.error(extractErrorMessage(error)),
  });

  const enabled = status?.enabled ?? false;
  const unavailable = status ? !status.available : false;
  const busy = toggle.isPending;
  const needsRetry =
    !!status &&
    ((status.enabled && status.inlineSecrets > 0) ||
      (!status.enabled && status.vaultedSecrets > status.unresolvedSecrets));

  return (
    <div className="space-y-3 rounded-lg border border-border bg-muted/40 p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-sm font-medium text-foreground">
            <KeyRound className="h-4 w-4" />
            {t("settings.secretsVault.title", {
              defaultValue: "Store API keys in the system keychain",
            })}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("settings.secretsVault.description", {
              defaultValue:
                "Moves provider API keys and tokens out of the CC Switch database into the system keychain. The database, its backups, SQL exports and cloud sync then hold only placeholders. The config files of Claude Code, Codex and other tools still contain the active key.",
            })}
          </p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={(checked) => toggle.mutate(checked)}
          disabled={busy || (!enabled && unavailable)}
          aria-label={t("settings.secretsVault.title", {
            defaultValue: "Store API keys in the system keychain",
          })}
        />
      </div>

      {!enabled && unavailable && (
        <p className="text-xs text-destructive" role="alert">
          {t("settings.secretsVault.unavailable", {
            defaultValue:
              "No system keychain is available on this device, so this option cannot be turned on. On Linux, run a Secret Service provider such as GNOME Keyring or KWallet.",
          })}
        </p>
      )}

      {enabled && (
        <p className="text-xs text-amber-600 dark:text-amber-400">
          {t("settings.secretsVault.exportWarning", {
            defaultValue:
              "SQL exports and cloud sync do not include the keys while this is on. On another device, re-enter the API keys; a device that already has a provider keeps its own key.",
          })}
        </p>
      )}

      {status && status.unresolvedSecrets > 0 && (
        <p className="text-xs text-destructive">
          {t("settings.secretsVault.unresolved", {
            defaultValue:
              "{{count}} API key(s) are not in this device's keychain. Edit these providers and enter the key again:",
            count: status.unresolvedSecrets,
          })}{" "}
          <span className="font-mono">
            {status.unresolvedProviders.join(", ")}
          </span>
        </p>
      )}

      {needsRetry && (
        <div className="flex items-center justify-between gap-4 text-xs text-muted-foreground">
          <span>
            {enabled
              ? t("settings.secretsVault.pendingIn", {
                  defaultValue:
                    "{{count}} key(s) are still stored in the database.",
                  count: status?.inlineSecrets ?? 0,
                })
              : t("settings.secretsVault.pendingOut", {
                  defaultValue: "Some keys are still in the keychain.",
                })}
          </span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => toggle.mutate(enabled)}
            disabled={busy}
          >
            {t("settings.secretsVault.retry", { defaultValue: "Retry" })}
          </Button>
        </div>
      )}
    </div>
  );
}
