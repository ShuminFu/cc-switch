import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { queryKeys, usageKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";

/** Mirrors `features/log_retention.rs` (PRODUCT-31). */
export interface RequestLogRetention {
  retentionDays: number;
  minDays: number;
  maxDays: number;
  databaseBytes?: number;
}

export const logRetentionApi = {
  get: () => invoke<RequestLogRetention>("get_request_log_retention"),
  set: (days: number) =>
    invoke<RequestLogRetention>("set_request_log_retention", { days }),
  pruneNow: () => invoke<number>("prune_request_logs_now"),
  getStreamCheck: () =>
    invoke<StreamCheckRetention>("get_stream_check_retention"),
  setStreamCheck: (days: number) =>
    invoke<StreamCheckRetention>("set_stream_check_retention", { days }),
};

/** Mirrors `StreamCheckRetention` in `features/log_retention.rs`; 0 = forever. */
export interface StreamCheckRetention {
  retentionDays: number;
}

const STREAM_CHECK_PRESETS = [1, 3, 7, 14, 30, 90, 0];

const PRESETS = [7, 14, 30, 90, 180, 365];

const formatBytes = (bytes: number) =>
  bytes < 1024 * 1024
    ? `${(bytes / 1024).toFixed(1)} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

/**
 * Settings → Data: how long per-request log rows are kept before they are
 * folded into daily totals, plus "Delete request logs now".
 */
export function RequestLogRetentionPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const { data } = useQuery({
    queryKey: queryKeys.logRetention.status,
    queryFn: logRetentionApi.get,
  });

  const { data: streamCheck } = useQuery({
    queryKey: queryKeys.logRetention.streamCheck,
    queryFn: logRetentionApi.getStreamCheck,
  });
  const saveStreamCheck = useMutation({
    mutationFn: logRetentionApi.setStreamCheck,
    onSuccess: (next) =>
      queryClient.setQueryData(queryKeys.logRetention.streamCheck, next),
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
  });
  const streamCheckDays = streamCheck?.retentionDays ?? 7;
  const streamCheckOptions = STREAM_CHECK_PRESETS.includes(streamCheckDays)
    ? STREAM_CHECK_PRESETS
    : [...STREAM_CHECK_PRESETS, streamCheckDays];

  const save = useMutation({
    mutationFn: logRetentionApi.set,
    onSuccess: (next) =>
      queryClient.setQueryData(queryKeys.logRetention.status, next),
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
  });

  const prune = useMutation({
    mutationFn: logRetentionApi.pruneNow,
    onSuccess: async (deleted) => {
      toast.success(
        t("logRetention.pruned", {
          defaultValue:
            "Deleted {{count}} request log rows; usage totals were kept",
          count: deleted,
        }),
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: usageKeys.all }),
        queryClient.invalidateQueries({
          queryKey: queryKeys.logRetention.status,
        }),
      ]);
    },
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
    onSettled: () => setConfirming(false),
  });

  const days = data?.retentionDays ?? 30;
  const options = PRESETS.includes(days)
    ? PRESETS
    : [...PRESETS, days].sort((a, b) => a - b);

  return (
    <div className="space-y-3 rounded-lg border border-border/60 p-4">
      <div>
        <h4 className="text-sm font-medium">
          {t("logRetention.title", { defaultValue: "Request log retention" })}
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("logRetention.description", {
            defaultValue:
              "Per-request metadata (model, tokens, status, session id, errors) is kept for this long, then folded into daily usage totals. Request and response bodies are never stored.",
          })}
        </p>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Select
          value={String(days)}
          onValueChange={(v) => save.mutate(Number(v))}
          disabled={!data || save.isPending}
        >
          <SelectTrigger
            className="w-40"
            aria-label={t("logRetention.title", {
              defaultValue: "Request log retention",
            })}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {options.map((n) => (
              <SelectItem key={n} value={String(n)}>
                {t("logRetention.days", {
                  defaultValue: "{{count}} days",
                  count: n,
                })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => setConfirming(true)}
          disabled={prune.isPending}
        >
          <Trash2 className="h-3.5 w-3.5" />
          {t("logRetention.deleteNow", {
            defaultValue: "Delete request logs now",
          })}
        </Button>
        {data?.databaseBytes != null && (
          <span className="text-xs text-muted-foreground">
            {t("logRetention.dbSize", {
              defaultValue: "Database size: {{size}}",
              size: formatBytes(data.databaseBytes),
            })}
          </span>
        )}
      </div>
      <div className="space-y-1.5 border-t border-border/60 pt-3">
        <h4 className="text-sm font-medium">
          {t("logRetention.streamCheckTitle", {
            defaultValue: "Connectivity check log retention",
          })}
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("logRetention.streamCheckDescription", {
            defaultValue:
              "Results of provider connectivity checks are deleted after this many days.",
          })}
        </p>
        <Select
          value={String(streamCheckDays)}
          onValueChange={(v) => saveStreamCheck.mutate(Number(v))}
          disabled={!streamCheck || saveStreamCheck.isPending}
        >
          <SelectTrigger
            className="w-40"
            aria-label={t("logRetention.streamCheckTitle", {
              defaultValue: "Connectivity check log retention",
            })}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {streamCheckOptions.map((n) => (
              <SelectItem key={n} value={String(n)}>
                {n === 0
                  ? t("logRetention.keepForever", {
                      defaultValue: "Keep forever",
                    })
                  : t("logRetention.days", {
                      defaultValue: "{{count}} days",
                      count: n,
                    })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <ConfirmDialog
        isOpen={confirming}
        isPending={prune.isPending}
        title={t("logRetention.confirmTitle", {
          defaultValue: "Delete request logs?",
        })}
        message={t("logRetention.confirmMessage", {
          defaultValue:
            "All per-request log rows are deleted after their tokens and costs are added to the daily totals. Usage charts keep their totals but request details are gone.",
        })}
        onConfirm={() => prune.mutate()}
        onCancel={() => setConfirming(false)}
      />
    </div>
  );
}
