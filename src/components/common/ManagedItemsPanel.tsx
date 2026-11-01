import React, { useMemo } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { AppId } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import { AppCountBar } from "./AppCountBar";
import { AppToggleGroup } from "./AppToggleGroup";
import { ListItemRow } from "./ListItemRow";
import type { ConfirmController } from "./useConfirm";

export interface ManagedItemAction {
  key: string;
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  tone?: "default" | "danger" | "info";
  disabled?: boolean;
  /** Show a spinner instead of the icon. */
  busy?: boolean;
  /**
   * Keep this action visible without hover/focus (e.g. "update available").
   * When any action of a row is pinned, the whole action bar is shown.
   */
  pinned?: boolean;
}

export interface ManagedItemsPanelProps<T> {
  items: T[];
  isLoading: boolean;
  getId: (item: T) => string;

  /**
   * Count bar. With `appIds` + `getApps`, per-app counts are derived from the
   * items and every row gets an app toggle group; `summary` replaces the
   * per-app badges for single-app lists (prompts).
   */
  totalLabel: string;
  summary?: React.ReactNode;
  appIds?: AppId[];
  getApps?: (item: T) => Partial<Record<AppId, boolean>>;
  onToggleApp?: (item: T, app: AppId, enabled: boolean) => void;
  /** Extra controls rendered next to the count bar. */
  toolbar?: React.ReactNode;
  /** Notices rendered between the count bar and the list (e.g. sync warnings). */
  banner?: React.ReactNode;

  /** Row slots. */
  renderLeading?: (item: T) => React.ReactNode;
  renderPrimary: (item: T) => React.ReactNode;
  renderSecondary?: (item: T) => React.ReactNode;
  getActions?: (item: T) => ManagedItemAction[];

  loadingLabel: string;
  empty: {
    icon: React.ReactNode;
    title: string;
    description: string;
  };

  /** Shared confirm dialog (from `useConfirm`). */
  confirm?: ConfirmController;
  /** Form / dialog slot rendered after the list (edit panel, import dialog). */
  children?: React.ReactNode;
}

const ACTION_TONE_CLASS: Record<
  NonNullable<ManagedItemAction["tone"]>,
  string
> = {
  default: "",
  danger:
    "hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10",
  info: "hover:text-blue-500 hover:bg-blue-100 dark:hover:text-blue-400 dark:hover:bg-blue-500/10",
};

/**
 * Shared list shell for MCP servers, Skills and Prompts (FE-UI-18): count
 * bar, loading / empty states, rows with per-app toggles and an action bar,
 * one confirm dialog and a form slot. Row actions are revealed on hover and
 * on keyboard focus (FE-UI-11).
 */
export function ManagedItemsPanel<T>({
  items,
  isLoading,
  getId,
  totalLabel,
  summary,
  appIds,
  getApps,
  onToggleApp,
  toolbar,
  banner,
  renderLeading,
  renderPrimary,
  renderSecondary,
  getActions,
  loadingLabel,
  empty,
  confirm,
  children,
}: ManagedItemsPanelProps<T>) {
  const counts = useMemo(() => {
    const result: Partial<Record<AppId, number>> = {};
    if (!appIds || !getApps) return result;
    for (const app of appIds) result[app] = 0;
    for (const item of items) {
      const apps = getApps(item);
      for (const app of appIds) {
        if (apps[app]) result[app] = (result[app] ?? 0) + 1;
      }
    }
    return result;
  }, [items, appIds, getApps]);

  const countBar =
    summary !== undefined ? (
      <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6">
        <div className="text-sm text-muted-foreground">{summary}</div>
      </div>
    ) : (
      <AppCountBar
        totalLabel={totalLabel}
        counts={counts}
        appIds={appIds ?? []}
      />
    );

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      {toolbar ? (
        <div className="flex items-center justify-between">
          {countBar}
          {toolbar}
        </div>
      ) : (
        countBar
      )}

      {banner}

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div
            className="text-center py-12 text-muted-foreground"
            role="status"
          >
            {loadingLabel}
          </div>
        ) : items.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              {empty.icon}
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {empty.title}
            </h3>
            <p className="text-muted-foreground text-sm">{empty.description}</p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <ul
              className="rounded-xl border border-border-default overflow-hidden"
              aria-label={totalLabel}
            >
              {items.map((item, index) => {
                const id = getId(item);
                const actions = getActions?.(item) ?? [];
                const pinned = actions.some((action) => action.pinned);
                return (
                  <li key={id}>
                    <ListItemRow isLast={index === items.length - 1}>
                      {renderLeading && (
                        <div className="flex-shrink-0">
                          {renderLeading(item)}
                        </div>
                      )}
                      <div className="flex-1 min-w-0">
                        {renderPrimary(item)}
                        {renderSecondary?.(item)}
                      </div>

                      {appIds && getApps && onToggleApp && (
                        <AppToggleGroup
                          apps={getApps(item)}
                          onToggle={(app, enabled) =>
                            onToggleApp(item, app, enabled)
                          }
                          appIds={appIds}
                        />
                      )}

                      {actions.length > 0 && (
                        <div
                          className={cn(
                            "flex items-center gap-0.5 flex-shrink-0 transition-opacity",
                            pinned
                              ? "opacity-100"
                              : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100",
                          )}
                        >
                          {actions.map((action) => (
                            <Button
                              key={action.key}
                              type="button"
                              variant="ghost"
                              size="icon"
                              className={cn(
                                "h-7 w-7",
                                ACTION_TONE_CLASS[action.tone ?? "default"],
                              )}
                              onClick={action.onClick}
                              disabled={action.disabled || action.busy}
                              title={action.label}
                              aria-label={action.label}
                            >
                              {action.busy ? (
                                <Loader2 size={14} className="animate-spin" />
                              ) : (
                                action.icon
                              )}
                            </Button>
                          ))}
                        </div>
                      )}
                    </ListItemRow>
                  </li>
                );
              })}
            </ul>
          </TooltipProvider>
        )}
      </div>

      {children}
      {confirm?.dialog}
    </div>
  );
}
