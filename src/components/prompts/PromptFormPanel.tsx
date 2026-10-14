import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Prompt, AppId } from "@/lib/api";
import { useEditableResource } from "@/hooks/useEditableResource";
import { useCloseGuard } from "@/hooks/useCloseGuard";
import { ExternalChangeBanner } from "@/components/common/ExternalChangeBanner";
import { DiscardChangesDialog } from "@/components/common/DiscardChangesDialog";

interface PromptDraft {
  name: string;
  description: string;
  content: string;
}

const toPromptDraft = (prompt: Prompt | null): PromptDraft => ({
  name: prompt?.name ?? "",
  description: prompt?.description ?? "",
  content: prompt?.content ?? "",
});

interface PromptFormPanelProps {
  appId: AppId;
  editingId?: string;
  initialData?: Prompt;
  onSave: (id: string, prompt: Prompt) => Promise<void>;
  onClose: () => void;
}

const PromptFormPanel: React.FC<PromptFormPanelProps> = ({
  appId,
  editingId,
  initialData,
  onSave,
  onClose,
}) => {
  const { t } = useTranslation();
  const appName = t(`apps.${appId}`);
  const filenameMap: Record<AppId, string> = {
    claude: "CLAUDE.md",
    "claude-desktop": "CLAUDE.md",
    codex: "AGENTS.md",
    gemini: "GEMINI.md",
    grokbuild: "AGENTS.md",
    opencode: "AGENTS.md",
    openclaw: "AGENTS.md",
    hermes: "AGENTS.md",
  };
  const filename = filenameMap[appId];
  // The parent's prompts map is replaced on every reload (toggle,
  // profile-applied, deep-link import). Those must never clobber in-progress
  // edits; a real content change while dirty shows Reload / Keep mine.
  const { draft, setDraft, isDirty, externalChange, reload, keepMine } =
    useEditableResource<Prompt | null, PromptDraft>({
      data: initialData ?? null,
      toDraft: toPromptDraft,
      initialDraft: toPromptDraft(null),
    });
  const { name, description, content } = draft;
  const setName = (value: string) =>
    setDraft((prev) => ({ ...prev, name: value }));
  const setDescription = (value: string) =>
    setDraft((prev) => ({ ...prev, description: value }));
  const setContent = (value: string) =>
    setDraft((prev) => ({ ...prev, content: value }));

  const closeGuard = useCloseGuard(isDirty, onClose);
  const [saving, setSaving] = useState(false);
  const [isDarkMode, setIsDarkMode] = useState(false);

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

  const handleSave = async () => {
    if (!name.trim()) {
      return;
    }

    setSaving(true);
    try {
      const id = editingId || `prompt-${Date.now()}`;
      const timestamp = Math.floor(Date.now() / 1000);
      const prompt: Prompt = {
        id,
        name: name.trim(),
        description: description.trim() || undefined,
        content: content.trim(),
        enabled: initialData?.enabled || false,
        createdAt: initialData?.createdAt || timestamp,
        updatedAt: timestamp,
      };
      await onSave(id, prompt);
      onClose();
    } catch (error) {
      // Error handled by hook
    } finally {
      setSaving(false);
    }
  };

  const title = editingId
    ? t("prompts.editTitle", { appName })
    : t("prompts.addTitle", { appName });

  return (
    <FullScreenPanel
      isOpen={true}
      title={title}
      onClose={closeGuard.requestClose}
      footer={
        <Button
          type="button"
          onClick={handleSave}
          disabled={!name.trim() || saving}
          className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      }
    >
      {externalChange && (
        <ExternalChangeBanner onReload={reload} onKeepMine={keepMine} />
      )}

      <div className="glass rounded-xl p-6 border border-white/10 space-y-6">
        <div>
          <Label htmlFor="name" className="text-foreground">
            {t("prompts.name")}
          </Label>
          <Input
            id="name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t("prompts.namePlaceholder")}
            className="mt-2"
          />
        </div>

        <div>
          <Label htmlFor="description" className="text-foreground">
            {t("prompts.description")}
          </Label>
          <Input
            id="description"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t("prompts.descriptionPlaceholder")}
            className="mt-2"
          />
        </div>

        <div>
          <Label htmlFor="content" className="block mb-2 text-foreground">
            {t("prompts.content")}
          </Label>
          <MarkdownEditor
            value={content}
            onChange={setContent}
            placeholder={t("prompts.contentPlaceholder", { filename })}
            darkMode={isDarkMode}
            minHeight="167px"
          />
        </div>
      </div>

      <DiscardChangesDialog
        isOpen={closeGuard.confirmOpen}
        onConfirm={closeGuard.confirmDiscard}
        onCancel={closeGuard.cancelDiscard}
      />
    </FullScreenPanel>
  );
};

export default PromptFormPanel;
