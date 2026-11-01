import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Prompt, AppId } from "@/lib/api";
import { useDarkMode } from "@/hooks/useDarkMode";

const PROMPT_FILENAMES: Record<AppId, string> = {
  claude: "CLAUDE.md",
  "claude-desktop": "CLAUDE.md",
  codex: "AGENTS.md",
  gemini: "GEMINI.md",
  grokbuild: "AGENTS.md",
  opencode: "AGENTS.md",
  openclaw: "AGENTS.md",
  hermes: "AGENTS.md",
};

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
  const filename = PROMPT_FILENAMES[appId];
  // Hydrated once per mount: the parent keys this panel by prompt id, so a
  // list refetch (profile switch, deep-link import) cannot wipe edits
  // (FE-UI-51).
  const [name, setName] = useState(initialData?.name ?? "");
  const [description, setDescription] = useState(
    initialData?.description ?? "",
  );
  const [content, setContent] = useState(initialData?.content ?? "");
  const [saving, setSaving] = useState(false);
  const isDarkMode = useDarkMode();

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
        // Stored as typed: trailing newlines are meaningful in Markdown files.
        content,
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
      onClose={onClose}
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
    </FullScreenPanel>
  );
};

export default PromptFormPanel;
