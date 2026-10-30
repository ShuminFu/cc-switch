import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { CloudRemoteSnapshotInfo } from "@/lib/api/settings";
import {
  detectSyncConflict,
  type SyncConflictKind,
} from "@/lib/errors/syncConflict";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { WebDavSyncStatus } from "@/types";
import type {
  ActionState,
  CloudSyncBackendAdapter,
  SyncFormValues,
} from "./types";
import { queryKeys } from "@/lib/query/keys";

export type TransferKind = "upload" | "download";

export type CloudSyncDialog =
  | { type: "upload"; info: CloudRemoteSnapshotInfo | null }
  | { type: "download"; info: CloudRemoteSnapshotInfo }
  | { type: "conflict"; operation: TransferKind; conflict: SyncConflictKind }
  | null;

export interface UseCloudSyncFormOptions {
  /** The user already acknowledged the auto-sync explanation. */
  autoSyncConfirmed?: boolean;
  /** Persists `autoSyncConfirmed` once the user acknowledged it. */
  onConfirmAutoSync?: () => Promise<unknown>;
}

const errorText = (error: unknown) =>
  extractErrorMessage(error) || String(error);

export function formatDbCompatVersion(version?: number | null): string | null {
  return typeof version === "number" ? `db-v${version}` : null;
}

/** Display text for the backend's "endpoint is plain HTTP" flag. */
function insecureTransportWarning(t: ReturnType<typeof useTranslation>["t"]) {
  return t("settings.cloudSync.insecureTransport", {
    defaultValue:
      "This endpoint uses plain HTTP: credentials and synced data are sent unencrypted. Use HTTPS unless this is a trusted local network.",
  });
}

/**
 * Form state machine shared by every cloud sync backend: local draft with
 * dirty/saved tracking, secret preservation across the redacted refetch,
 * test/save, and the upload/download flows (preview dialog → transfer with
 * conflict detection → explicit overwrite).
 */
export function useCloudSyncForm<
  F extends SyncFormValues,
  C extends { status?: WebDavSyncStatus },
>(
  adapter: CloudSyncBackendAdapter<F, C>,
  config: C | undefined,
  options: UseCloudSyncFormOptions = {},
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const m = adapter.messages;
  const { autoSyncConfirmed, onConfirmAutoSync } = options;

  const [form, setForm] = useState<F>(() => adapter.fromConfig(config));
  const [dirty, setDirty] = useState(false);
  const [secretTouched, setSecretTouched] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const [actionState, setActionState] = useState<ActionState>("idle");
  const [dialog, setDialog] = useState<CloudSyncDialog>(null);
  const [showAutoSyncConfirm, setShowAutoSyncConfirm] = useState(false);
  const justSavedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Secret typed before a save, kept for the one refetch that follows it
  // (the refetched config has the secret redacted).
  const pendingSecretRef = useRef<{ key: string; secret: string } | null>(null);

  useEffect(
    () => () => {
      if (justSavedTimerRef.current) clearTimeout(justSavedTimerRef.current);
    },
    [],
  );

  // Adopt the persisted config when it loads or changes, unless editing.
  useEffect(() => {
    if (!config || dirty) return;
    const next = adapter.fromConfig(config);
    const pending = pendingSecretRef.current;
    pendingSecretRef.current = null;
    if (
      !next[adapter.secretField] &&
      pending?.secret &&
      pending.key === adapter.secretScopeKey(next)
    ) {
      (next as Record<string, string | boolean>)[adapter.secretField] =
        pending.secret;
    }
    setForm((prev) => ({
      ...next,
      // A preset the config cannot express stays as the user picked it.
      preset: adapter.presetIsPersisted ? next.preset : prev.preset,
    }));
    setSecretTouched(false);
  }, [adapter, config, dirty]);

  const markDirty = useCallback(() => {
    setDirty(true);
    setJustSaved(false);
    if (justSavedTimerRef.current) {
      clearTimeout(justSavedTimerRef.current);
      justSavedTimerRef.current = null;
    }
  }, []);

  const setField = useCallback(
    (name: keyof F & string, value: string | boolean) => {
      if (name === "autoSync" && value === true && !autoSyncConfirmed) {
        setShowAutoSyncConfirm(true);
        return;
      }
      setForm((prev) => ({ ...prev, [name]: value }));
      if (name === adapter.secretField) setSecretTouched(true);
      markDirty();
    },
    [adapter.secretField, autoSyncConfirmed, markDirty],
  );

  const blurField = useCallback(
    (name: keyof F & string) => {
      const spec = adapter.fields.find(
        (entry) => entry !== "presetHint" && entry.name === name,
      );
      if (!spec || spec === "presetHint" || !spec.onBlur) return;
      setForm((prev) => {
        const patch = spec.onBlur?.(prev);
        return patch ? { ...prev, ...patch } : prev;
      });
    },
    [adapter.fields],
  );

  const changePreset = useCallback(
    (presetId: string) => {
      let changed = false;
      setForm((prev) => {
        const result = adapter.applyPreset(prev, presetId);
        changed = result.changed;
        return result.form;
      });
      if (changed) markDirty();
    },
    [adapter, markDirty],
  );

  const confirmAutoSync = useCallback(async () => {
    setShowAutoSyncConfirm(false);
    await onConfirmAutoSync?.();
    setForm((prev) => ({ ...prev, autoSync: true }));
    markDirty();
  }, [markDirty, onConfirmAutoSync]);

  const cancelAutoSync = useCallback(() => setShowAutoSyncConfirm(false), []);

  const buildSettings = useCallback((): C | null => {
    const settings = adapter.toSettings(form, secretTouched);
    if (!settings) toast.error(t(m.missingRequired));
    return settings;
  }, [adapter, form, m.missingRequired, secretTouched, t]);

  const runTest = useCallback(
    async (settings: C, preserveEmptySecret: boolean, afterSave: boolean) => {
      setActionState("testing");
      try {
        const result = await adapter.api.test(settings, preserveEmptySecret);
        toast.success(t(afterSave ? m.saveAndTestSuccess : m.testSuccess));
        if (result?.insecureTransport) {
          toast.warning(insecureTransportWarning(t));
        }
      } catch (error) {
        const message = afterSave ? m.saveAndTestFailed : m.testFailed;
        const text = t(message, { error: errorText(error) });
        if (afterSave) toast.warning(text);
        else toast.error(text);
      } finally {
        setActionState("idle");
      }
    },
    [adapter.api, m, t],
  );

  const test = useCallback(async () => {
    const settings = buildSettings();
    if (!settings) return;
    await runTest(settings, !secretTouched, false);
  }, [buildSettings, runTest, secretTouched]);

  const save = useCallback(async () => {
    const settings = buildSettings();
    if (!settings) return;
    setActionState("saving");
    const secret = form[adapter.secretField];
    pendingSecretRef.current =
      typeof secret === "string" && secret
        ? { key: adapter.secretScopeKey(form), secret }
        : null;
    try {
      await adapter.api.save(settings, secretTouched);
      setDirty(false);
      setSecretTouched(false);
      setJustSaved(true);
      if (justSavedTimerRef.current) clearTimeout(justSavedTimerRef.current);
      justSavedTimerRef.current = setTimeout(() => {
        setJustSaved(false);
        justSavedTimerRef.current = null;
      }, 2000);
      await queryClient.invalidateQueries({ queryKey: queryKeys.settings.all });
    } catch (error) {
      pendingSecretRef.current = null;
      toast.error(t(m.saveFailed, { error: errorText(error) }));
      setActionState("idle");
      return;
    }
    await runTest(settings, true, true);
  }, [
    adapter,
    buildSettings,
    form,
    m.saveFailed,
    queryClient,
    runTest,
    secretTouched,
    t,
  ]);

  const blockIfDirty = useCallback(() => {
    if (!dirty) return false;
    toast.error(t(m.unsavedChanges));
    return true;
  }, [dirty, m.unsavedChanges, t]);

  const closeDialog = useCallback(() => setDialog(null), []);

  /** Fetch the remote snapshot, then open the upload preview dialog. */
  const startUpload = useCallback(async () => {
    if (blockIfDirty()) return;
    setActionState("fetching_remote");
    try {
      const info = await adapter.api.fetchRemoteInfo();
      setDialog({ type: "upload", info: "empty" in info ? null : info });
    } catch {
      toast.error(t(m.fetchRemoteFailed));
    } finally {
      setActionState("idle");
    }
  }, [adapter.api, blockIfDirty, m.fetchRemoteFailed, t]);

  /** Fetch the remote snapshot, check it, then open the download dialog. */
  const startDownload = useCallback(async () => {
    if (blockIfDirty()) return;
    setActionState("fetching_remote");
    try {
      const info = await adapter.api.fetchRemoteInfo();
      if ("empty" in info) {
        toast.info(t(m.noRemoteData));
        return;
      }
      if (!info.compatible) {
        toast.error(
          t(m.incompatibleVersion, {
            version: info.version,
            protocolVersion: info.protocolVersion ?? info.version,
            dbCompatVersion:
              formatDbCompatVersion(info.dbCompatVersion) ??
              t("common.unknown"),
          }),
        );
        return;
      }
      setDialog({ type: "download", info });
    } catch (error) {
      toast.error(t(m.downloadFailed, { error: errorText(error) }));
    } finally {
      setActionState("idle");
    }
  }, [adapter.api, blockIfDirty, m, t]);

  /**
   * Run a transfer. The first attempt is non-forced so the backend refuses
   * when the other side changed since the last sync; the conflict dialog then
   * lets the user retry with `force`.
   */
  const transfer = useCallback(
    async (operation: TransferKind, force: boolean) => {
      if (blockIfDirty()) return;
      setDialog(null);
      setActionState(operation === "upload" ? "uploading" : "downloading");
      try {
        const run =
          operation === "upload" ? adapter.api.upload : adapter.api.download;
        const result = await run({ force });
        toast.success(
          t(operation === "upload" ? m.uploadSuccess : m.downloadSuccess),
        );
        if (result?.warning) toast.warning(result.warning);
        // A download replaces the whole database, so every cached query is
        // stale; an upload only changes the sync status in settings.
        await queryClient.invalidateQueries(
          operation === "download"
            ? undefined
            : { queryKey: queryKeys.settings.all },
        );
      } catch (error) {
        const conflict = force ? null : detectSyncConflict(error);
        if (conflict) {
          setDialog({ type: "conflict", operation, conflict });
        } else {
          toast.error(
            t(operation === "upload" ? m.uploadFailed : m.downloadFailed, {
              error: errorText(error),
            }),
          );
        }
      } finally {
        setActionState("idle");
      }
    },
    [adapter.api, blockIfDirty, m, queryClient, t],
  );

  const confirmUpload = useCallback(
    () => transfer("upload", false),
    [transfer],
  );
  const confirmDownload = useCallback(
    () => transfer("download", false),
    [transfer],
  );
  const forceTransfer = useCallback(
    () =>
      dialog?.type === "conflict"
        ? transfer(dialog.operation, true)
        : Promise.resolve(),
    [dialog, transfer],
  );

  return {
    form,
    dirty,
    justSaved,
    actionState,
    dialog,
    showAutoSyncConfirm,
    insecureTransportWarning: insecureTransportWarning(t),
    setField,
    blurField,
    changePreset,
    confirmAutoSync,
    cancelAutoSync,
    test,
    save,
    startUpload,
    startDownload,
    confirmUpload,
    confirmDownload,
    forceTransfer,
    closeDialog,
  };
}
