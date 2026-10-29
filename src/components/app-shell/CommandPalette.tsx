import { useEffect, useMemo, useRef } from "react";
import { useQueries } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { defaultFilter } from "cmdk";
import { Check, Plus, Radio, Settings as SettingsIcon } from "lucide-react";
import type { Provider, VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { fetchProvidersForApp, STALE_TIME } from "@/lib/query";
import { providerKeys } from "@/entities/provider/keys";
import { appCaps, appLabel, useAppRegistry } from "@/entities/app/registry";
import { APP_ICON_MAP } from "@/config/appConfig";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  visibleApps: VisibleApps;
  activeApp: AppId;
  /** 切换到指定应用的供应商（仅非累加模式应用、且不是当前供应商时触发） */
  onSwitchProvider: (appId: AppId, provider: Provider) => void;
  /** 跳转到指定应用的供应商列表（累加模式应用或已是当前供应商时触发） */
  onSelectApp: (appId: AppId) => void;
  onOpenSettings: () => void;
  onAddProvider: () => void;
  /** 未传入时不显示代理接管开关（与头部 ProxyToggle 的显示条件保持一致） */
  onToggleTakeover?: () => void;
  isTakeoverActive: boolean;
}

// 只按 keywords（应用名 + 供应商名）匹配，避免 value 中的供应商 ID（常为 UUID）
// 被模糊匹配命中
const filterByKeywords = (
  value: string,
  search: string,
  keywords?: string[],
): number =>
  defaultFilter(keywords?.length ? keywords.join(" ") : value, search);

/**
 * Cmd/Ctrl+K 命令面板：列出所有可见应用的供应商与常用操作。
 * 应用列表来自 `useAppRegistry()`，供应商数据与 `useProvidersQuery` 共用
 * `providerKeys.list(appId)` 缓存。
 */
export function CommandPalette({
  open,
  onOpenChange,
  visibleApps,
  activeApp,
  onSwitchProvider,
  onSelectApp,
  onOpenSettings,
  onAddProvider,
  onToggleTakeover,
  isTakeoverActive,
}: CommandPaletteProps) {
  const { t } = useTranslation();
  const registry = useAppRegistry();
  const apps = useMemo(
    () => registry.filter((app) => visibleApps[app.id]).map((app) => app.id),
    [registry, visibleApps],
  );

  const results = useQueries({
    queries: apps.map((appId) => ({
      queryKey: providerKeys.list(appId),
      queryFn: () => fetchProvidersForApp(appId),
      staleTime: STALE_TIME.short,
      enabled: open,
    })),
  });

  const run = (action: () => void) => {
    onOpenChange(false);
    action();
  };

  const activeLabel = appLabel(activeApp);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        zIndex="top"
        className="overflow-hidden p-0 max-w-xl"
        onInteractOutside={() => onOpenChange(false)}
      >
        <DialogTitle className="sr-only">
          {t("commandPalette.title")}
        </DialogTitle>
        <DialogDescription className="sr-only">
          {t("commandPalette.description")}
        </DialogDescription>
        <Command filter={filterByKeywords} loop>
          <CommandInput
            placeholder={t("commandPalette.placeholder")}
            aria-label={t("commandPalette.placeholder")}
          />
          <CommandList className="max-h-[360px]">
            <CommandEmpty>{t("commandPalette.empty")}</CommandEmpty>
            {apps.map((appId, index) => {
              const data = results[index]?.data;
              const providers = Object.values(data?.providers ?? {});
              if (providers.length === 0) return null;
              const label = appLabel(appId);
              const additive = appCaps(appId).additive;
              return (
                <CommandGroup key={appId} heading={label}>
                  {providers.map((provider) => {
                    const isCurrent =
                      !additive && provider.id === data?.currentProviderId;
                    return (
                      <CommandItem
                        key={provider.id}
                        value={`provider:${appId}:${provider.id}`}
                        keywords={[label, provider.name]}
                        onSelect={() =>
                          run(() =>
                            additive || isCurrent
                              ? onSelectApp(appId)
                              : onSwitchProvider(appId, provider),
                          )
                        }
                      >
                        <span className="flex h-4 w-4 items-center justify-center">
                          {APP_ICON_MAP[appId]?.icon}
                        </span>
                        <span className="flex-1 truncate">{provider.name}</span>
                        {isCurrent && (
                          <span className="flex items-center gap-1 text-xs text-muted-foreground">
                            <Check />
                            {t("commandPalette.current")}
                          </span>
                        )}
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              );
            })}
            <CommandGroup heading={t("commandPalette.actions")}>
              <CommandItem
                value="action:add-provider"
                keywords={[
                  t("commandPalette.addProvider", { app: activeLabel }),
                ]}
                onSelect={() => run(onAddProvider)}
              >
                <Plus />
                {t("commandPalette.addProvider", { app: activeLabel })}
              </CommandItem>
              {onToggleTakeover && (
                <CommandItem
                  value="action:toggle-takeover"
                  keywords={[
                    isTakeoverActive
                      ? t("commandPalette.disableTakeover", {
                          app: activeLabel,
                        })
                      : t("commandPalette.enableTakeover", {
                          app: activeLabel,
                        }),
                  ]}
                  onSelect={() => run(onToggleTakeover)}
                >
                  <Radio />
                  {isTakeoverActive
                    ? t("commandPalette.disableTakeover", { app: activeLabel })
                    : t("commandPalette.enableTakeover", { app: activeLabel })}
                </CommandItem>
              )}
              <CommandItem
                value="action:open-settings"
                keywords={[t("commandPalette.openSettings")]}
                onSelect={() => run(onOpenSettings)}
              >
                <SettingsIcon />
                {t("commandPalette.openSettings")}
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}

const isPaletteShortcut = (event: KeyboardEvent): boolean =>
  (event.metaKey || event.ctrlKey) &&
  !event.shiftKey &&
  !event.altKey &&
  event.key.toLowerCase() === "k";

/**
 * 全局 Cmd/Ctrl+K：打开 / 关闭命令面板。
 * 已被编辑器等处理（defaultPrevented）或已有其他弹窗打开时不响应打开。
 */
export function useCommandPaletteShortcut(
  open: boolean,
  setOpen: (open: boolean) => void,
): void {
  const stateRef = useRef({ open, setOpen });

  useEffect(() => {
    stateRef.current = { open, setOpen };
  }, [open, setOpen]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat) return;
      if (!isPaletteShortcut(event)) return;

      const { open: isOpen, setOpen: set } = stateRef.current;
      if (
        !isOpen &&
        document.querySelector('[role="dialog"], [role="alertdialog"]')
      ) {
        return;
      }

      event.preventDefault();
      set(!isOpen);
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);
}
