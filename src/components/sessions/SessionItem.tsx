import {
  Archive,
  ArchiveRestore,
  ChevronRight,
  Clock,
  Pin,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SessionMeta } from "@/types";
import type { SessionAnnotation } from "@/lib/api/sessionAnnotations";
import {
  formatRelativeTime,
  formatSessionTitle,
  getProviderIconName,
  getProviderLabel,
  getSessionKey,
  highlightText,
} from "./utils";

interface SessionItemProps {
  session: SessionMeta;
  isSelected: boolean;
  selectionMode: boolean;
  isChecked: boolean;
  isCheckDisabled?: boolean;
  searchQuery?: string;
  /** Transcript excerpt from the backend full-text search. */
  snippet?: string;
  onSelect: (key: string) => void;
  onToggleChecked: (checked: boolean) => void;
  /** Pin / archive / tags (SESS-18); omitted = no annotation controls. */
  annotation?: SessionAnnotation;
  onAnnotate?: (patch: Partial<SessionAnnotation>) => void;
}

export function SessionItem({
  session,
  isSelected,
  selectionMode,
  isChecked,
  isCheckDisabled = false,
  searchQuery,
  snippet,
  onSelect,
  onToggleChecked,
  annotation,
  onAnnotate,
}: SessionItemProps) {
  const { t } = useTranslation();
  const title = formatSessionTitle(session);
  const lastActive = session.lastActiveAt || session.createdAt || undefined;
  const sessionKey = getSessionKey(session);

  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg px-3 py-2.5 transition-all group",
        isSelected
          ? "bg-primary/10 border border-primary/30"
          : "hover:bg-muted/60 border border-transparent",
      )}
    >
      {selectionMode && (
        <div className="shrink-0 pt-0.5">
          <Checkbox
            checked={isChecked}
            disabled={isCheckDisabled}
            aria-label={t("sessionManager.selectForBatch", {
              defaultValue: "选择会话",
            })}
            onCheckedChange={(checked) => onToggleChecked(Boolean(checked))}
          />
        </div>
      )}
      <button
        type="button"
        onClick={() => onSelect(sessionKey)}
        className="min-w-0 flex-1 text-left"
      >
        <div className="flex items-center gap-2 mb-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="shrink-0">
                <ProviderIcon
                  icon={getProviderIconName(session.providerId)}
                  name={session.providerId}
                  size={18}
                />
              </span>
            </TooltipTrigger>
            <TooltipContent>
              {getProviderLabel(session.providerId, t)}
            </TooltipContent>
          </Tooltip>
          {annotation?.pinned ? (
            <Pin
              className="size-3.5 shrink-0 text-primary"
              aria-label={t("sessionManager.annotations.pinned", {
                defaultValue: "Pinned",
              })}
            />
          ) : null}
          <span className="text-sm font-medium line-clamp-2 flex-1">
            {searchQuery ? highlightText(title, searchQuery) : title}
          </span>
          <ChevronRight
            className={cn(
              "size-4 text-muted-foreground/50 shrink-0 transition-transform",
              isSelected && "text-primary rotate-90",
            )}
          />
        </div>

        {snippet ? (
          <p className="mb-1 line-clamp-2 text-[11px] text-muted-foreground">
            {searchQuery ? highlightText(snippet, searchQuery) : snippet}
          </p>
        ) : null}
        <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
          <Clock className="size-3" />
          <span>
            {lastActive
              ? formatRelativeTime(lastActive, t)
              : t("common.unknown")}
          </span>
          {annotation?.archived ? (
            <span className="ml-1 rounded border px-1 text-[10px]">
              {t("sessionManager.annotations.archivedBadge", {
                defaultValue: "Archived",
              })}
            </span>
          ) : null}
          {annotation?.tags.map((tag) => (
            <span key={tag} className="ml-1 rounded bg-muted px-1 text-[10px]">
              #{tag}
            </span>
          ))}
        </div>
      </button>
      {onAnnotate && !selectionMode ? (
        <div className="flex shrink-0 flex-col gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <button
            type="button"
            className={cn(
              "rounded p-0.5 text-muted-foreground hover:text-foreground",
              annotation?.pinned && "text-primary opacity-100",
            )}
            aria-pressed={annotation?.pinned ?? false}
            aria-label={
              annotation?.pinned
                ? t("sessionManager.annotations.unpin", {
                    defaultValue: "Unpin session",
                  })
                : t("sessionManager.annotations.pin", {
                    defaultValue: "Pin session",
                  })
            }
            onClick={() => onAnnotate({ pinned: !annotation?.pinned })}
          >
            <Pin className="size-3.5" />
          </button>
          <button
            type="button"
            className="rounded p-0.5 text-muted-foreground hover:text-foreground"
            aria-label={
              annotation?.archived
                ? t("sessionManager.annotations.unarchive", {
                    defaultValue: "Unarchive session",
                  })
                : t("sessionManager.annotations.archive", {
                    defaultValue: "Archive session",
                  })
            }
            onClick={() => onAnnotate({ archived: !annotation?.archived })}
          >
            {annotation?.archived ? (
              <ArchiveRestore className="size-3.5" />
            ) : (
              <Archive className="size-3.5" />
            )}
          </button>
        </div>
      ) : null}
    </div>
  );
}
