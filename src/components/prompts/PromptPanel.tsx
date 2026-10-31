import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Edit3, FileText, Trash2 } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { promptsApi, type AppId, type Prompt } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { queryKeys } from "@/lib/query/keys";
import { ManagedItemsPanel } from "@/components/common/ManagedItemsPanel";
import { useConfirm } from "@/components/common/useConfirm";
import PromptFormPanel from "./PromptFormPanel";
import PromptToggle from "./PromptToggle";

interface PromptPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
}

export interface PromptPanelHandle {
  openAdd: () => void;
}

type PromptEntry = [string, Prompt];
const getEntryId = ([id]: PromptEntry) => id;

const PromptPanel = React.forwardRef<PromptPanelHandle, PromptPanelProps>(
  ({ appId }, ref) => {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const [isFormOpen, setIsFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const confirm = useConfirm();

    const {
      prompts,
      loading,
      savePrompt,
      deletePrompt,
      toggleEnabled,
      isToggling,
    } = usePromptActions(appId);

    // Applying a project profile switches the enabled prompt.
    useTauriEvent("profile-applied", () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.prompts.all });
    });

    const handleAdd = () => {
      setEditingId(null);
      setIsFormOpen(true);
    };

    React.useImperativeHandle(ref, () => ({
      openAdd: handleAdd,
    }));

    const handleEdit = (id: string) => {
      setEditingId(id);
      setIsFormOpen(true);
    };

    const handleDelete = (id: string) => {
      const prompt = prompts[id];
      confirm.confirm({
        title: t("prompts.confirm.deleteTitle"),
        message: t("prompts.confirm.deleteMessage", { name: prompt?.name }),
        // The hook already toasts on failure; a rejection keeps the dialog open.
        onConfirm: () => deletePrompt(id),
      });
    };

    const promptEntries = useMemo(() => Object.entries(prompts), [prompts]);
    const enabledPrompt = promptEntries.find(([, p]) => p.enabled);

    // FE-UI-46: detect a live prompt file (CLAUDE.md / AGENTS.md / ...) that
    // was edited outside the app since the enabled prompt was written.
    const { data: liveContent } = useQuery({
      queryKey: queryKeys.prompts.currentFile(appId),
      queryFn: () => promptsApi.getCurrentFileContent(appId),
      enabled: !!enabledPrompt,
    });
    const liveDiffers =
      !!enabledPrompt &&
      typeof liveContent === "string" &&
      liveContent.trim() !== (enabledPrompt[1].content ?? "").trim();
    const [isImportingLive, setIsImportingLive] = useState(false);

    const importLiveChanges = async () => {
      if (!enabledPrompt || typeof liveContent !== "string") return;
      const [id, prompt] = enabledPrompt;
      setIsImportingLive(true);
      try {
        await savePrompt(id, {
          ...prompt,
          content: liveContent,
          updatedAt: Math.floor(Date.now() / 1000),
        });
      } catch {
        // savePrompt already reported the failure.
      } finally {
        setIsImportingLive(false);
      }
    };

    return (
      <ManagedItemsPanel
        items={promptEntries}
        isLoading={loading}
        getId={getEntryId}
        totalLabel={t("prompts.count", { count: promptEntries.length })}
        summary={
          <>
            {t("prompts.count", { count: promptEntries.length })} ·{" "}
            {enabledPrompt
              ? t("prompts.enabledName", { name: enabledPrompt[1].name })
              : t("prompts.noneEnabled")}
            {liveDiffers && (
              <div
                className="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-300"
                role="status"
                data-testid="prompt-live-drift"
              >
                <span className="flex items-center gap-1.5">
                  <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                  {t("prompts.liveFileDiffers", {
                    defaultValue:
                      "The live prompt file was edited outside CC Switch and differs from the enabled prompt. Re-enabling or editing the prompt overwrites those changes.",
                  })}
                </span>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  disabled={isImportingLive}
                  onClick={() => void importLiveChanges()}
                >
                  {t("prompts.importLiveChanges", {
                    defaultValue: "Import live changes",
                  })}
                </Button>
              </div>
            )}
          </>
        }
        renderLeading={([id, prompt]) => (
          <PromptToggle
            enabled={prompt.enabled === true}
            disabled={isToggling}
            label={prompt.name}
            onChange={(next) => toggleEnabled(id, next)}
          />
        )}
        renderPrimary={([, prompt]) => (
          <span className="block font-medium text-sm text-foreground truncate">
            {prompt.name}
          </span>
        )}
        renderSecondary={([, prompt]) =>
          prompt.description ? (
            <p
              className="text-xs text-muted-foreground truncate"
              title={prompt.description}
            >
              {prompt.description}
            </p>
          ) : null
        }
        getActions={([id]) => [
          {
            key: "edit",
            label: t("common.edit"),
            icon: <Edit3 size={14} />,
            onClick: () => handleEdit(id),
          },
          {
            key: "delete",
            label: t("common.delete"),
            icon: <Trash2 size={14} />,
            tone: "danger",
            onClick: () => handleDelete(id),
          },
        ]}
        loadingLabel={t("prompts.loading")}
        empty={{
          icon: <FileText size={24} className="text-muted-foreground" />,
          title: t("prompts.empty"),
          description: t("prompts.emptyDescription"),
        }}
        confirm={confirm}
      >
        {isFormOpen && (
          <PromptFormPanel
            // Remount per prompt so the form hydrates once per edit session
            // and list refetches never overwrite in-progress edits (FE-UI-51).
            key={editingId ?? "new"}
            appId={appId}
            editingId={editingId || undefined}
            initialData={editingId ? prompts[editingId] : undefined}
            onSave={savePrompt}
            onClose={() => setIsFormOpen(false)}
          />
        )}
      </ManagedItemsPanel>
    );
  },
);

PromptPanel.displayName = "PromptPanel";

export default PromptPanel;
