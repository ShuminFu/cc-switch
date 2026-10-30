import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { workspaceApi } from "@/lib/api/workspace";
import { useDarkMode } from "@/hooks/useDarkMode";

interface WorkspaceFileEditorProps {
  filename: string;
  isOpen: boolean;
  onClose: () => void;
}

const WorkspaceFileEditor: React.FC<WorkspaceFileEditorProps> = ({
  filename,
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(true);
  // Only a successful read may enable saving: after a failed read the buffer
  // is empty (or still holds a previously opened file), and saving it would
  // overwrite the real file.
  const [loadError, setLoadError] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);
  const [saving, setSaving] = useState(false);
  const isDarkMode = useDarkMode();

  useEffect(() => {
    if (!isOpen || !filename) return;

    let active = true;
    setLoading(true);
    setLoadError(false);
    setContent("");
    workspaceApi
      .readFile(filename)
      .then((data) => {
        if (!active) return;
        setContent(data ?? "");
      })
      .catch((err) => {
        if (!active) return;
        console.error("Failed to read workspace file:", err);
        setLoadError(true);
        toast.error(t("workspace.loadFailed"));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [isOpen, filename, t, reloadToken]);

  const canSave = !saving && !loading && !loadError;

  const handleSave = useCallback(async () => {
    if (loading || loadError) return;
    setSaving(true);
    try {
      await workspaceApi.writeFile(filename, content);
      toast.success(t("workspace.saveSuccess"));
    } catch (err) {
      console.error("Failed to save workspace file:", err);
      toast.error(t("workspace.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [filename, content, t, loading, loadError]);

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("workspace.editing", { filename })}
      onClose={onClose}
      footer={
        <Button onClick={handleSave} disabled={!canSave}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
          {t("prompts.loading")}
        </div>
      ) : loadError ? (
        <div className="flex flex-col items-center justify-center gap-3 h-64 text-muted-foreground">
          <p>{t("workspace.loadFailed")}</p>
          <Button
            variant="outline"
            onClick={() => setReloadToken((value) => value + 1)}
          >
            {t("common.retry", { defaultValue: "Retry" })}
          </Button>
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
    </FullScreenPanel>
  );
};

export default WorkspaceFileEditor;
