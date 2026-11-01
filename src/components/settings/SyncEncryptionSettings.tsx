import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  syncEncryptionApi,
  type SyncEncryptionStatus,
} from "@/lib/api/syncEncryption";
import { extractErrorMessage } from "@/utils/errorUtils";
import { queryKeys } from "@/lib/query/keys";

const QUERY_KEY = queryKeys.sync.encryptionStatus;

/**
 * End-to-end encryption of the WebDAV/S3 sync snapshot (SEC-06 / SVC-04).
 * The passphrase is write-only: it is never read back into the webview.
 */
export function SyncEncryptionSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");

  const { data: status } = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => syncEncryptionApi.getStatus(),
  });

  const onStatus = (next: SyncEncryptionStatus) => {
    queryClient.setQueryData(QUERY_KEY, next);
    setEditing(false);
    setPassphrase("");
    setConfirm("");
  };

  const save = useMutation({
    mutationFn: (value: string) => syncEncryptionApi.setPassphrase(value),
    onSuccess: (next) => {
      onStatus(next);
      toast.success(
        t("settings.syncEncryption.saved", {
          defaultValue:
            "Sync encryption is on. The next upload will be encrypted.",
        }),
      );
    },
    onError: (error) => toast.error(extractErrorMessage(error)),
  });

  const disable = useMutation({
    mutationFn: () => syncEncryptionApi.disable(),
    onSuccess: (next) => {
      onStatus(next);
      toast.success(
        t("settings.syncEncryption.disabled", {
          defaultValue: "Sync encryption is off.",
        }),
      );
    },
    onError: (error) => toast.error(extractErrorMessage(error)),
  });

  const enabled = status?.enabled ?? false;
  const minLength = status?.minPassphraseLength ?? 8;
  const showForm = editing;
  const tooShort = [...passphrase].length < minLength;
  const mismatch = passphrase !== confirm;
  const busy = save.isPending || disable.isPending;

  const handleToggle = (checked: boolean) => {
    if (checked) {
      setEditing(true);
    } else if (enabled) {
      disable.mutate();
    } else {
      setEditing(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-muted/40 p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-sm font-medium text-foreground">
            <Lock className="h-4 w-4" />
            {t("settings.syncEncryption.title", {
              defaultValue: "End-to-end encryption",
            })}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("settings.syncEncryption.description", {
              defaultValue:
                "Encrypt the synced database (including API keys) and skills with a passphrase before upload. Every device needs the same passphrase. If you lose it, the remote copy cannot be recovered. Update CC Switch on all devices first.",
            })}
          </p>
        </div>
        <Switch
          checked={enabled || editing}
          onCheckedChange={handleToggle}
          disabled={busy}
          aria-label={t("settings.syncEncryption.title", {
            defaultValue: "End-to-end encryption",
          })}
        />
      </div>

      {enabled && !editing && (
        <div className="flex items-center justify-between gap-4 text-xs text-muted-foreground">
          <span>
            {status?.storage === "keychain"
              ? t("settings.syncEncryption.storedInKeychain", {
                  defaultValue: "Passphrase stored in the system keychain.",
                })
              : status?.hasPassphrase
                ? t("settings.syncEncryption.storedInFile", {
                    defaultValue:
                      "No system keychain available: passphrase stored in settings.json (owner-only).",
                  })
                : t("settings.syncEncryption.unavailable", {
                    defaultValue:
                      "The passphrase cannot be read from the system keychain. Uploads are paused until you enter it again.",
                  })}
          </span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => setEditing(true)}
            disabled={busy}
          >
            {t("settings.syncEncryption.change", {
              defaultValue: "Change passphrase",
            })}
          </Button>
        </div>
      )}

      {showForm && (
        <form
          className="space-y-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (!tooShort && !mismatch) save.mutate(passphrase);
          }}
        >
          <Input
            type="password"
            autoComplete="new-password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            placeholder={t("settings.syncEncryption.passphrase", {
              defaultValue: "Passphrase",
            })}
            className="text-xs"
          />
          <Input
            type="password"
            autoComplete="new-password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            placeholder={t("settings.syncEncryption.confirm", {
              defaultValue: "Repeat passphrase",
            })}
            className="text-xs"
          />
          {passphrase !== "" && tooShort && (
            <p className="text-xs text-destructive">
              {t("settings.syncEncryption.tooShort", {
                defaultValue: "Use at least {{count}} characters.",
                count: minLength,
              })}
            </p>
          )}
          {!tooShort && confirm !== "" && mismatch && (
            <p className="text-xs text-destructive">
              {t("settings.syncEncryption.mismatch", {
                defaultValue: "The passphrases do not match.",
              })}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => {
                setEditing(false);
                setPassphrase("");
                setConfirm("");
              }}
              disabled={busy}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={busy || tooShort || mismatch}
            >
              {t("settings.syncEncryption.save", {
                defaultValue: "Save passphrase",
              })}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
