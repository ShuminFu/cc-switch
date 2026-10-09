import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Check, Zap } from "lucide-react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";
import type { Provider } from "@/types";

interface QuickSwitcherProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 当前应用的显示名，用于标题 */
  appLabel: string;
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSelect: (provider: Provider) => void;
}

/** 按主页排序（sortIndex，其次名称）排列供应商 */
export function sortProvidersForSwitcher(
  providers: Record<string, Provider>,
): Provider[] {
  return Object.values(providers).sort((a, b) => {
    const ai = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
    const bi = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
    if (ai !== bi) return ai - bi;
    return a.name.localeCompare(b.name);
  });
}

/**
 * ⌘K / Ctrl+K 快速切换：键盘筛选当前应用的供应商并回车切换。
 * 不做任何额外确认——和点击主页卡片是同一条切换路径。
 */
export function QuickSwitcher({
  open,
  onOpenChange,
  appLabel,
  providers,
  currentProviderId,
  onSelect,
}: QuickSwitcherProps) {
  const { t } = useTranslation();
  const sorted = useMemo(
    () => sortProvidersForSwitcher(providers),
    [providers],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="overflow-hidden p-0 sm:max-w-md"
        data-testid="quick-switcher"
      >
        <DialogTitle className="sr-only">
          {t("quickSwitch.title", {
            defaultValue: "快速切换 {{app}} 供应商",
            app: appLabel,
          })}
        </DialogTitle>
        <Command loop>
          <CommandInput
            autoFocus
            placeholder={t("quickSwitch.placeholder", {
              defaultValue: "搜索 {{app}} 供应商…",
              app: appLabel,
            })}
          />
          <CommandList className="max-h-80">
            <CommandEmpty>
              {t("quickSwitch.empty", { defaultValue: "没有匹配的供应商" })}
            </CommandEmpty>
            <CommandGroup>
              {sorted.map((provider) => {
                const isCurrent = provider.id === currentProviderId;
                return (
                  <CommandItem
                    key={provider.id}
                    value={provider.id}
                    keywords={[provider.name, provider.websiteUrl ?? ""]}
                    onSelect={() => onSelect(provider)}
                    data-testid={`quick-switch-${provider.id}`}
                  >
                    <ProviderIcon
                      icon={provider.icon}
                      name={provider.name}
                      size={16}
                      className="mr-2 shrink-0"
                    />
                    <span className="truncate">{provider.name}</span>
                    {isCurrent && (
                      <span className="ml-auto flex items-center gap-1 pl-2 text-xs text-muted-foreground">
                        <Check className="h-3.5 w-3.5" />
                        {t("quickSwitch.current", { defaultValue: "当前" })}
                      </span>
                    )}
                  </CommandItem>
                );
              })}
            </CommandGroup>
          </CommandList>
          <div
            className={cn(
              "flex items-center gap-1.5 border-t border-border/60 px-3 py-1.5 text-[11px] text-muted-foreground",
            )}
          >
            <Zap className="h-3 w-3" />
            {t("quickSwitch.hint", {
              defaultValue: "↑↓ 选择 · Enter 切换 · Esc 关闭",
            })}
          </div>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
