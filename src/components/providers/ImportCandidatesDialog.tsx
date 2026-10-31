import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { providersApi, type AppId } from "@/lib/api";
import type { ImportCandidate } from "@/lib/bindings";
import { useImportCandidates } from "@/lib/query/adapters";
import { queryKeys } from "@/lib/query/keys";
import { showError } from "@/lib/errors/showError";
import type { Provider } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

/** Exclusive apps whose adapter lists live providers (`get_import_candidates`). */
export const IMPORT_PICKER_APPS: readonly AppId[] = [
  "claude",
  "codex",
  "gemini",
];

export const supportsImportPicker = (appId: AppId): boolean =>
  IMPORT_PICKER_APPS.includes(appId);

/** `idHint`, or `idHint-2`, `idHint-3`… when that id is taken. */
export function uniqueProviderId(
  idHint: string,
  taken: ReadonlySet<string>,
): string {
  const base = idHint.trim() || "imported";
  if (!taken.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!taken.has(candidate)) return candidate;
  }
}

interface ImportCandidatesDialogProps {
  open: boolean;
  appId: AppId;
  /** Ids already in the list; matching candidates are marked and unchecked. */
  existingIds: readonly string[];
  onOpenChange: (open: boolean) => void;
}

/**
 * "Import from live config" picker (CFG-23): lists every provider the app's
 * live config defines and imports the selected ones into the database.
 * Candidates carry credentials, so the query is not cached after closing.
 */
export function ImportCandidatesDialog({
  open,
  appId,
  existingIds,
  onOpenChange,
}: ImportCandidatesDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: candidates, isLoading, isError, error } = useImportCandidates(
    appId,
    open,
  );
  const existing = useMemo(() => new Set(existingIds), [existingIds]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [isImporting, setIsImporting] = useState(false);

  useEffect(() => {
    if (!candidates) return;
    setSelected(
      new Set(
        candidates
          .filter((candidate) => !existing.has(candidate.idHint))
          .map((candidate) => candidate.fingerprint),
      ),
    );
  }, [candidates, existing]);

  const toggle = (candidate: ImportCandidate, checked: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(candidate.fingerprint);
      else next.delete(candidate.fingerprint);
      return next;
    });
  };

  const handleImport = async () => {
    if (!candidates) return;
    const chosen = candidates.filter((c) => selected.has(c.fingerprint));
    if (chosen.length === 0) return;
    setIsImporting(true);
    const taken = new Set(existing);
    let imported = 0;
    try {
      for (const candidate of chosen) {
        const id = uniqueProviderId(candidate.idHint, taken);
        taken.add(id);
        const provider: Provider = {
          id,
          name: candidate.name || id,
          settingsConfig: candidate.settingsConfig as Provider["settingsConfig"],
          createdAt: Date.now(),
        };
        await providersApi.add(provider, appId, false);
        imported += 1;
      }
      toast.success(
        t("provider.importPicker.done", {
          defaultValue: "Imported {{count}} provider(s)",
          count: imported,
        }),
      );
      onOpenChange(false);
    } catch (importError) {
      showError({
        title: t("provider.importPicker.failed", {
          defaultValue: "Import failed after {{count}} provider(s)",
          count: imported,
        }),
        error: importError,
      });
    } finally {
      setIsImporting(false);
      if (imported > 0) {
        await queryClient.invalidateQueries({
          queryKey: queryKeys.providers.byApp(appId),
        });
      }
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!isImporting) onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-lg" zIndex="nested">
        <DialogHeader>
          <DialogTitle>
            {t("provider.importPicker.title", {
              defaultValue: "Import from live config",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.importPicker.description", {
              defaultValue:
                "Providers defined in the app's current config files. Select the ones to add to CC Switch.",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[50vh] space-y-2 overflow-y-auto px-6 py-4">
          {isLoading && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("common.loading")}
            </div>
          )}
          {isError && (
            <p role="alert" className="text-sm text-destructive">
              {extractErrorMessage(error) ||
                t("provider.importPicker.readFailed", {
                  defaultValue: "Could not read the live config.",
                })}
            </p>
          )}
          {candidates && candidates.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("provider.importPicker.none", {
                defaultValue: "No providers found in the live config.",
              })}
            </p>
          )}
          {candidates?.map((candidate) => {
            const already = existing.has(candidate.idHint);
            const checkboxId = `import-candidate-${candidate.fingerprint}`;
            return (
              <label
                key={candidate.fingerprint}
                htmlFor={checkboxId}
                className="flex cursor-pointer items-start gap-3 rounded-md border border-border-default p-3"
              >
                <Checkbox
                  id={checkboxId}
                  checked={selected.has(candidate.fingerprint)}
                  onCheckedChange={(value) => toggle(candidate, value === true)}
                  aria-label={candidate.name || candidate.idHint}
                  className="mt-0.5"
                />
                <span className="min-w-0 space-y-0.5">
                  <span className="block truncate text-sm font-medium">
                    {candidate.name || candidate.idHint}
                  </span>
                  <span className="block truncate font-mono text-xs text-muted-foreground">
                    {candidate.idHint}
                    {already &&
                      ` · ${t("provider.importPicker.alreadyAdded", {
                        defaultValue: "already in the list",
                      })}`}
                  </span>
                </span>
              </label>
            );
          })}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isImporting}
          >
            {t("common.cancel")}
          </Button>
          <Button
            onClick={() => void handleImport()}
            disabled={isImporting || selected.size === 0}
            aria-busy={isImporting || undefined}
          >
            {t("provider.importPicker.import", {
              defaultValue: "Import selected ({{count}})",
              count: selected.size,
            })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
