import { useState, type FormEvent } from "react";
import { Tag } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

/** Same limits as the backend (`session_manager/annotations.rs`). */
export const MAX_SESSION_TAGS = 20;
export const MAX_SESSION_TAG_CHARS = 40;

/** Split "a, b #c" into unique trimmed tags (case-insensitive), capped. */
export function parseSessionTags(raw: string): string[] {
  const seen = new Set<string>();
  const tags: string[] = [];
  for (const part of raw.split(/[,\n]/)) {
    const tag = part.trim().replace(/^#+/, "").slice(0, MAX_SESSION_TAG_CHARS);
    if (!tag || seen.has(tag.toLowerCase())) continue;
    seen.add(tag.toLowerCase());
    tags.push(tag);
    if (tags.length >= MAX_SESSION_TAGS) break;
  }
  return tags;
}

interface SessionTagEditorProps {
  tags: string[];
  onSave: (tags: string[]) => void;
}

/** Tag button + popover editor for one session (SESS-18). */
export function SessionTagEditor({ tags, onSave }: SessionTagEditorProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");

  const label = t("sessionManager.annotations.editTags", {
    defaultValue: "Edit tags",
  });

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault();
    onSave(parseSessionTags(draft));
    setOpen(false);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) setDraft(tags.join(", "));
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded p-0.5 text-muted-foreground hover:text-foreground"
          aria-label={label}
          title={label}
        >
          <Tag className="size-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-64 p-3" align="end">
        <form className="space-y-2" onSubmit={handleSubmit}>
          <Input
            autoFocus
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            aria-label={t("sessionManager.annotations.tagsInput", {
              defaultValue: "Tags, separated by commas",
            })}
            placeholder={t("sessionManager.annotations.tagsPlaceholder", {
              defaultValue: "bugfix, client-a",
            })}
            className="h-8 text-xs"
          />
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" size="sm">
              {t("common.save")}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}
