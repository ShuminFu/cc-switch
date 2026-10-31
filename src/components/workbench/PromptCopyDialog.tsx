import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Copy } from "lucide-react";
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
import { APP_REGISTRY, appsWith } from "@/config/apps";
import type { AppId } from "@/lib/api/types";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import { promptShareApi } from "./api";

interface PromptCopyDialogProps {
  appId: AppId;
  promptId: string | null;
  promptName?: string;
  onClose: () => void;
}

/**
 * Copy one prompt to other apps (CLAUDE.md, AGENTS.md, GEMINI.md, …) and
 * optionally make it the active prompt there (PRODUCT-59).
 */
export function PromptCopyDialog({
  appId,
  promptId,
  promptName,
  onClose,
}: PromptCopyDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const sourceFeatureApp = APP_REGISTRY[appId].featureApp;
  const candidates = appsWith("prompts").filter(
    (id) => id !== appId && id !== sourceFeatureApp && id !== "claude-desktop",
  );
  const [targets, setTargets] = useState<Set<AppId>>(new Set());
  const [activate, setActivate] = useState(true);

  const copy = useMutation({
    mutationFn: () =>
      promptShareApi.copy(sourceFeatureApp, promptId!, [...targets], activate),
    onSuccess: async (results) => {
      const failed = results.filter((r) => r.status === "error");
      const ok = results.length - failed.length;
      if (failed.length > 0) {
        toast.error(failed.map((r) => `${r.app}: ${r.error ?? ""}`).join("\n"));
      }
      if (ok > 0) {
        toast.success(
          t("workbench.promptCopy.done", {
            defaultValue: "Prompt copied to {{count}} apps",
            count: ok,
          }),
        );
      }
      await queryClient.invalidateQueries({ queryKey: queryKeys.prompts.all });
      onClose();
    },
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
  });

  return (
    <Dialog
      open={promptId !== null}
      onOpenChange={(open) => {
        if (!open && !copy.isPending) onClose();
      }}
    >
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>
            {t("workbench.promptCopy.title", {
              defaultValue: "Use in other apps",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("workbench.promptCopy.description", {
              defaultValue:
                'Copy "{{name}}" to the prompt lists of other apps. An existing prompt file that was edited outside CC Switch is kept as a backup prompt.',
              name: promptName ?? promptId ?? "",
            })}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2 px-6">
          {candidates.map((id) => (
            <label key={id} className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={targets.has(id)}
                onCheckedChange={(v) =>
                  setTargets((s) => {
                    const next = new Set(s);
                    if (v === true) next.add(id);
                    else next.delete(id);
                    return next;
                  })
                }
              />
              {APP_REGISTRY[id].label}
            </label>
          ))}
          <label className="flex items-center gap-2 border-t border-border/50 pt-2 text-sm">
            <Checkbox
              checked={activate}
              onCheckedChange={(v) => setActivate(v === true)}
            />
            {t("workbench.promptCopy.activate", {
              defaultValue: "Make it the active prompt there",
            })}
          </label>
        </div>
        <DialogFooter className="flex gap-2 sm:justify-end">
          <Button variant="outline" onClick={onClose} disabled={copy.isPending}>
            {t("common.cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button
            onClick={() => copy.mutate()}
            disabled={copy.isPending || targets.size === 0 || !promptId}
          >
            <Copy className="mr-1 h-3.5 w-3.5" />
            {t("workbench.promptCopy.copy", { defaultValue: "Copy" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
