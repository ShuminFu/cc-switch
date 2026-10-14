import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ExternalChangeBanner } from "@/components/common/ExternalChangeBanner";
import { DiscardChangesDialog } from "@/components/common/DiscardChangesDialog";
import { workspaceApi } from "@/lib/api/workspace";
import { useEditableResource } from "@/hooks/useEditableResource";
import { useCloseGuard } from "@/hooks/useCloseGuard";

interface WorkspaceFileEditorProps {
  filename: string;
  isOpen: boolean;
  onClose: () => void;
}

interface WorkspaceFileSnapshot {
  filename: string;
  content: string;
}

const toContent = (file: WorkspaceFileSnapshot) => file.content;

const WorkspaceFileEditor: React.FC<WorkspaceFileEditorProps> = ({
  filename,
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();
  const [file, setFile] = useState<WorkspaceFileSnapshot | undefined>();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [isDarkMode, setIsDarkMode] = useState(false);

  const {
    draft: content,
    setDraft: setContent,
    isDirty,
    externalChange,
    reload,
    keepMine,
    markSaved,
  } = useEditableResource<WorkspaceFileSnapshot, string>({
    data: file,
    toDraft: toContent,
    initialDraft: "",
  });

  // Discarding on close reverts the draft so reopening starts clean.
  const closeGuard = useCloseGuard(isDirty, onClose, reload);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));
    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!isOpen || !filename) return;

    // Ignore results from a previous filename/open cycle: otherwise a slow
    // read of file A can land after file B was opened and the user would
    // then save A's content into B.
    let cancelled = false;
    const read = (initial: boolean) => {
      if (initial) setLoading(true);
      workspaceApi
        .readFile(filename)
        .then((data) => {
          if (!cancelled) setFile({ filename, content: data ?? "" });
        })
        .catch((err) => {
          if (cancelled || !initial) return;
          console.error("Failed to read workspace file:", err);
          toast.error(t("workspace.loadFailed"));
        })
        .finally(() => {
          if (!cancelled && initial) setLoading(false);
        });
    };

    read(true);
    // Pick up edits made by the agent or another editor while we were away.
    // useEditableResource re-seeds a clean editor and shows the
    // Reload / Keep mine banner for a dirty one.
    const handleFocus = () => read(false);
    window.addEventListener("focus", handleFocus);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", handleFocus);
    };
  }, [isOpen, filename, t]);

  // Never save one file's buffer into another (e.g. if reading the newly
  // opened file failed and the previous file's content is still loaded).
  const fileLoaded = file?.filename === filename;

  const handleSave = useCallback(async () => {
    const snapshot = content;
    setSaving(true);
    try {
      await workspaceApi.writeFile(filename, snapshot);
      markSaved(snapshot);
      toast.success(t("workspace.saveSuccess"));
    } catch (err) {
      console.error("Failed to save workspace file:", err);
      toast.error(t("workspace.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [filename, content, markSaved, t]);

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("workspace.editing", { filename })}
      onClose={closeGuard.requestClose}
      footer={
        <Button
          onClick={() => void handleSave()}
          disabled={saving || loading || !fileLoaded}
        >
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      }
    >
      {externalChange && (
        <ExternalChangeBanner onReload={reload} onKeepMine={keepMine} />
      )}
      {loading ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
          {t("prompts.loading")}
        </div>
      ) : (
        <MarkdownEditor
          value={content}
          onChange={setContent}
          darkMode={isDarkMode}
          placeholder={`# ${filename}\n\n...`}
          minHeight="calc(100vh - 240px)"
        />
      )}
      <DiscardChangesDialog
        isOpen={closeGuard.confirmOpen}
        onConfirm={closeGuard.confirmDiscard}
        onCancel={closeGuard.cancelDiscard}
      />
    </FullScreenPanel>
  );
};

export default WorkspaceFileEditor;
