import { ChevronRight, Clock, Coins } from "lucide-react";
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
import type { SessionUsageStat } from "@/types/usage";
import {
  formatRelativeTime,
  formatSessionCost,
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
  /** 该会话归集到的费用；无记录时不显示 */
  usage?: SessionUsageStat;
  onSelect: (key: string) => void;
  onToggleChecked: (checked: boolean) => void;
}

export function SessionItem({
  session,
  isSelected,
  selectionMode,
  isChecked,
  isCheckDisabled = false,
  searchQuery,
  usage,
  onSelect,
  onToggleChecked,
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

        <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
          <Clock className="size-3" />
          <span>
            {lastActive
              ? formatRelativeTime(lastActive, t)
              : t("common.unknown")}
          </span>
          {usage && (
            <>
              <span className="text-muted-foreground/40">·</span>
              <span
                className="flex items-center gap-0.5"
                title={t("sessionManager.usageCostTooltip", {
                  defaultValue: "会话估算费用",
                })}
                data-testid="session-item-cost"
              >
                <Coins className="size-3" />
                {formatSessionCost(usage.totalCostUsd)}
              </span>
            </>
          )}
        </div>
      </button>
    </div>
  );
}
