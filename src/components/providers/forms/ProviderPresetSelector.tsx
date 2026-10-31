import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import {
  ArrowUpAZ,
  Search,
  Zap,
  Star,
  Heart,
  Layers,
  Settings2,
  EyeOff,
} from "lucide-react";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import type { OpenClawProviderPreset } from "@/config/openclawProviderPresets";
import type { HermesProviderPreset } from "@/config/hermesProviderPresets";
import type { ProviderCategory } from "@/types";
import {
  universalProviderPresets,
  type UniversalProviderPreset,
} from "@/config/universalProviderPresets";
import { ProviderIcon } from "@/components/ProviderIcon";
import { settingsApi } from "@/lib/api/settings";
import { HintText } from "./shared/HintText";

type PresetTranslator = (key: string) => unknown;

export const PresetSortMode = {
  Original: "original",
  NameAsc: "nameAsc",
} as const;

export type PresetSortMode =
  (typeof PresetSortMode)[keyof typeof PresetSortMode];

export type AnyPreset =
  | ProviderPreset
  | CodexProviderPreset
  | GeminiProviderPreset
  | ClaudeDesktopProviderPreset
  | OpenCodeProviderPreset
  | OpenClawProviderPreset
  | HermesProviderPreset;

export type PresetEntry = {
  id: string;
  preset: AnyPreset;
};

/** Public sponsor / affiliate policy linked from the preset picker (PRODUCT-08). */
export const SPONSOR_POLICY_URL =
  "https://github.com/farion1231/cc-switch/blob/main/docs/sponsor-policy.md";

/** localStorage key for the "hide sponsored presets" preference. */
export const HIDE_SPONSORED_STORAGE_KEY = "cc-switch:presets:hideSponsored";

/** Partner and prime-partner presets are paid placements and may carry referral links. */
export function isSponsoredPreset(preset: AnyPreset): boolean {
  return Boolean(preset.isPartner || preset.primePartner);
}

export function readHideSponsoredPreference(): boolean {
  try {
    return globalThis.localStorage?.getItem(HIDE_SPONSORED_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeHideSponsoredPreference(hide: boolean): void {
  try {
    if (hide) {
      globalThis.localStorage?.setItem(HIDE_SPONSORED_STORAGE_KEY, "1");
    } else {
      globalThis.localStorage?.removeItem(HIDE_SPONSORED_STORAGE_KEY);
    }
  } catch {
    // storage unavailable: the preference simply does not persist
  }
}

export function getPresetDisplayName(
  preset: AnyPreset,
  t: PresetTranslator,
): string {
  return preset.nameKey ? String(t(preset.nameKey)) : preset.name;
}

export function getPresetSearchText(
  entry: PresetEntry,
  t: PresetTranslator,
): string {
  return [getPresetDisplayName(entry.preset, t), entry.preset.name]
    .join(" ")
    .toLowerCase();
}

export function filterPresetEntries(
  entries: PresetEntry[],
  query: string,
  t: PresetTranslator,
): PresetEntry[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return entries;
  }

  return entries.filter((entry) =>
    getPresetSearchText(entry, t).includes(normalizedQuery),
  );
}

export function sortPresetEntries(
  entries: PresetEntry[],
  sortMode: PresetSortMode,
  t: PresetTranslator,
): PresetEntry[] {
  if (sortMode === PresetSortMode.Original) {
    // 置顶优先级：官方分类 > 尊享合作伙伴（Kimi）> 其余原顺序。
    // 用分区拼接而非排序，确保每组内部各自的相对顺序都不变；
    // 排他条件保证「既是官方又是 prime」的预设只归入官方组、不被重复。
    const official = entries.filter(
      (entry) => entry.preset.category === "official",
    );
    const prime = entries.filter(
      (entry) =>
        entry.preset.category !== "official" && entry.preset.primePartner,
    );
    const rest = entries.filter(
      (entry) =>
        entry.preset.category !== "official" && !entry.preset.primePartner,
    );
    return [...official, ...prime, ...rest];
  }

  return [...entries].sort((a, b) =>
    getPresetDisplayName(a.preset, t).localeCompare(
      getPresetDisplayName(b.preset, t),
    ),
  );
}

export interface PresetVisibilityOptions {
  query: string;
  sortMode: PresetSortMode;
  t: PresetTranslator;
  /** Drop sponsored (partner / prime partner) presets. */
  hideSponsored?: boolean;
}

export function getVisiblePresetEntries(
  entries: PresetEntry[],
  options: PresetVisibilityOptions,
): PresetEntry[] {
  const { query, sortMode, t, hideSponsored = false } = options;
  const candidates = hideSponsored
    ? entries.filter((entry) => !isSponsoredPreset(entry.preset))
    : entries;

  return sortPresetEntries(
    filterPresetEntries(candidates, query, t),
    sortMode,
    t,
  );
}

interface ProviderPresetSelectorProps {
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  presetCategoryLabels: Record<string, string>;
  onPresetChange: (value: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  category?: ProviderCategory; // 当前选中的分类
}

export function ProviderPresetSelector({
  selectedPresetId,
  presetEntries,
  presetCategoryLabels,
  onPresetChange,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  category,
}: Readonly<ProviderPresetSelectorProps>) {
  const { t } = useTranslation();
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortMode, setSortMode] = useState<PresetSortMode>(
    PresetSortMode.Original,
  );
  const [hideSponsored, setHideSponsored] = useState(
    readHideSponsoredPreference,
  );
  const searchContainerRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // 点击搜索区域外时收起并清空,对齐旧 Popover 的「点击外部关闭」行为
  useEffect(() => {
    if (!searchOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (
        searchContainerRef.current &&
        !searchContainerRef.current.contains(event.target as Node)
      ) {
        setSearchOpen(false);
        setSearchQuery("");
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [searchOpen]);

  // 键盘快捷键: Ctrl/Cmd+F 打开搜索并聚焦输入框。
  // 使用捕获阶段并阻止冒泡，避免背后 ProviderList 的同名快捷键被意外触发。
  // 首次打开靠 Input 的 autoFocus 聚焦；若搜索已打开（例如点击 preset 后焦点
  // 停在按钮上），setSearchOpen(true) 同值不会重渲染、autoFocus 不重触发，
  // 这里用 rAF 命令式地把焦点移回搜索框（不 select，避免吞掉随后输入的首字符）。
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        setSearchOpen(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
      }
    };

    globalThis.addEventListener("keydown", handleKeyDown, true);
    return () => globalThis.removeEventListener("keydown", handleKeyDown, true);
  }, []);

  const visiblePresetEntries = useMemo(
    () =>
      getVisiblePresetEntries(presetEntries, {
        query: searchQuery,
        sortMode,
        t,
        hideSponsored,
      }),
    [presetEntries, searchQuery, sortMode, t, hideSponsored],
  );

  const hasSponsoredPresets = useMemo(
    () => presetEntries.some((entry) => isSponsoredPreset(entry.preset)),
    [presetEntries],
  );

  const toggleHideSponsored = () => {
    setHideSponsored((current) => {
      const next = !current;
      writeHideSponsoredPreference(next);
      return next;
    });
  };

  const sponsoredTooltip = t("providerPreset.sponsoredTooltip", {
    defaultValue:
      "Sponsored placement: this partner supports CC Switch, and its sign-up links may earn CC Switch a referral commission.",
  });

  const openSponsorPolicy = () => {
    void settingsApi.openExternal(SPONSOR_POLICY_URL).catch((error) => {
      console.error("[ProviderPresetSelector] Failed to open policy", error);
    });
  };

  const hint = (text: string) => <HintText>{text}</HintText>;

  const getCategoryHint = (): ReactNode => {
    switch (category) {
      case "official":
        return hint(
          t("providerForm.officialHint", {
            defaultValue: "💡 Official provider uses browser login, no API Key needed",
          }),
        );
      case "cn_official":
        return hint(
          t("providerForm.cnOfficialApiKeyHint", {
            defaultValue: "💡 Only need to fill in API Key, endpoint is preset",
          }),
        );
      case "aggregator":
        return hint(
          t("providerForm.aggregatorApiKeyHint", {
            defaultValue: "💡 Only need to fill in API Key, endpoint is preset",
          }),
        );
      case "third_party":
        return hint(
          t("providerForm.thirdPartyApiKeyHint", {
            defaultValue: "💡 Only need to fill in API Key, endpoint is preset",
          }),
        );
      case "custom":
        return hint(
          t("providerForm.customApiKeyHint", {
            defaultValue: "💡 Custom configuration requires manually filling all necessary fields",
          }),
        );
      case "omo":
        return hint(
          t("providerForm.omoHint", {
            defaultValue:
              "💡 OMO config manages Agent model assignments and supports both oh-my-openagent.jsonc and oh-my-opencode.jsonc",
          }),
        );
      default:
        return t("providerPreset.hint", {
          defaultValue: "You can continue to adjust the fields below after selecting a preset.",
        });
    }
  };

  const toggleSortMode = () => {
    setSortMode((current) =>
      current === PresetSortMode.Original
        ? PresetSortMode.NameAsc
        : PresetSortMode.Original,
    );
  };

  const renderPresetIcon = (preset: AnyPreset) => {
    if (preset.icon) {
      return (
        <ProviderIcon
          icon={preset.icon}
          name={preset.name}
          color={preset.iconColor}
          size={16}
          className="flex-shrink-0"
        />
      );
    }

    const iconType = preset.theme?.icon;
    if (iconType) {
      switch (iconType) {
        case "claude":
          return <ClaudeIcon size={14} />;
        case "codex":
          return <CodexIcon size={14} />;
        case "gemini":
          return <GeminiIcon size={14} />;
        case "generic":
          return <Zap size={14} />;
      }
    }

    return <span className="inline-block w-4 h-4 flex-shrink-0" aria-hidden />;
  };

  const getPresetButtonClass = (isSelected: boolean, preset: AnyPreset) => {
    const baseClass =
      "inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors w-full";

    if (isSelected) {
      if (preset.theme?.backgroundColor) {
        return `${baseClass} text-white`;
      }
      return `${baseClass} bg-blue-500 text-white dark:bg-blue-600`;
    }

    return `${baseClass} bg-accent text-muted-foreground hover:bg-accent/80`;
  };

  const getPresetButtonStyle = (isSelected: boolean, preset: AnyPreset) => {
    if (!isSelected || !preset.theme?.backgroundColor) {
      return undefined;
    }

    return {
      backgroundColor: preset.theme.backgroundColor,
      color: preset.theme.textColor || "#FFFFFF",
    };
  };

  return (
    <div ref={searchContainerRef} className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <FormLabel>{t("providerPreset.label")}</FormLabel>
        <div className="flex items-center gap-2">
          {searchOpen && (
            <Input
              ref={searchInputRef}
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setSearchQuery("");
                  setSearchOpen(false);
                }
              }}
              placeholder={t("providerPreset.searchPlaceholder", {
                defaultValue: "Search presets...",
              })}
              aria-label={t("providerPreset.searchAriaLabel", {
                defaultValue: "Search provider presets",
              })}
              className="w-60 h-8"
              autoFocus
            />
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t("providerPreset.searchAriaLabel", {
              defaultValue: "Search provider presets",
            })}
            aria-pressed={searchOpen}
            onClick={() => {
              setSearchOpen((v) => !v);
              if (searchOpen) setSearchQuery("");
            }}
            title={t("providerPreset.searchTooltip", {
              defaultValue: "Search presets",
            })}
            className={
              searchOpen || searchQuery.trim()
                ? "size-8 bg-accent text-foreground"
                : "size-8"
            }
          >
            <Search className="size-4" />
          </Button>

          {hasSponsoredPresets && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label={t("providerPreset.hideSponsoredAriaLabel", {
                defaultValue: "Hide sponsored presets",
              })}
              aria-pressed={hideSponsored}
              onClick={toggleHideSponsored}
              title={
                hideSponsored
                  ? t("providerPreset.showSponsoredTooltip", {
                      defaultValue: "Show sponsored presets",
                    })
                  : t("providerPreset.hideSponsoredTooltip", {
                      defaultValue: "Hide sponsored presets",
                    })
              }
              className={
                hideSponsored ? "size-8 bg-accent text-foreground" : "size-8"
              }
            >
              <EyeOff className="size-4" />
            </Button>
          )}

          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t("providerPreset.sortAriaLabel", {
              defaultValue: "Toggle preset sorting",
            })}
            aria-pressed={sortMode === PresetSortMode.NameAsc}
            onClick={toggleSortMode}
            title={
              sortMode === PresetSortMode.NameAsc
                ? t("providerPreset.sortOriginalTooltip", {
                    defaultValue: "Restore original order",
                  })
                : t("providerPreset.sortNameAscTooltip", {
                    defaultValue: "Sort A-Z",
                  })
            }
            className={
              sortMode === PresetSortMode.NameAsc
                ? "size-8 bg-accent text-foreground"
                : "size-8"
            }
          >
            <ArrowUpAZ className="size-4" />
          </Button>
        </div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
        <button
          type="button"
          onClick={() => onPresetChange("custom")}
          className={`inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors w-full ${
            selectedPresetId === "custom"
              ? "bg-blue-500 text-white dark:bg-blue-600"
              : "bg-accent text-muted-foreground hover:bg-accent/80"
          }`}
        >
          <span className="inline-block w-4 h-4 flex-shrink-0" aria-hidden />
          <span className="truncate">{t("providerPreset.custom")}</span>
        </button>

        {visiblePresetEntries.length === 0 && (
          <div className="col-span-full rounded-md border border-dashed border-border-default px-3 py-2 text-xs text-muted-foreground">
            {t("providerPreset.noSearchResults", {
              defaultValue: "No matching presets.",
            })}
          </div>
        )}

        {visiblePresetEntries.map((entry) => {
          const isSelected = selectedPresetId === entry.id;
          const isPartner = entry.preset.isPartner;
          const isPrimePartner = entry.preset.primePartner;
          const presetCategory = entry.preset.category ?? "others";
          const categoryLabel =
            presetCategoryLabels[presetCategory] ?? t("providerPreset.other");
          const sponsored = isSponsoredPreset(entry.preset);
          return (
            <button
              key={entry.id}
              type="button"
              onClick={() => onPresetChange(entry.id)}
              className={`${getPresetButtonClass(isSelected, entry.preset)} relative`}
              style={getPresetButtonStyle(isSelected, entry.preset)}
              title={
                sponsored
                  ? `${categoryLabel} · ${sponsoredTooltip}`
                  : categoryLabel
              }
              data-sponsored={sponsored ? "true" : undefined}
            >
              {renderPresetIcon(entry.preset)}
              <span className="truncate">
                {getPresetDisplayName(entry.preset, t)}
              </span>
              {isPrimePartner ? (
                <Heart
                  className="absolute -top-1 -right-1 h-5 w-5 fill-amber-500 text-amber-500 drop-shadow-sm"
                  strokeWidth={0}
                  aria-hidden
                />
              ) : (
                isPartner && (
                  <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-amber-500 to-yellow-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                    <Star className="h-2.5 w-2.5 fill-current" />
                  </span>
                )
              )}
            </button>
          );
        })}
      </div>

      {onUniversalPresetSelect && universalProviderPresets.length > 0 && (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
          {universalProviderPresets.map((preset) => (
            <button
              key={`universal-${preset.providerType}`}
              type="button"
              onClick={() => onUniversalPresetSelect(preset)}
              className="inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80 relative w-full"
              title={t("universalProvider.hint", {
                defaultValue: "Cross-app unified config, auto-sync to Claude/Codex/Gemini",
              })}
            >
              <ProviderIcon
                icon={preset.icon}
                name={preset.name}
                size={14}
                className="flex-shrink-0"
              />
              <span className="truncate">{preset.name}</span>
              <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-indigo-500 to-purple-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                <Layers className="h-2.5 w-2.5" />
              </span>
            </button>
          ))}
          {onManageUniversalProviders && (
            <button
              type="button"
              onClick={onManageUniversalProviders}
              className="inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80 w-full"
              title={t("universalProvider.manage", {
                defaultValue: "Manage",
              })}
            >
              <Settings2 className="h-4 w-4 flex-shrink-0" />
              <span className="truncate">
                {t("universalProvider.manage", {
                  defaultValue: "Manage",
                })}
              </span>
            </button>
          )}
        </div>
      )}

      <p className="text-xs text-muted-foreground">{getCategoryHint()}</p>

      {hasSponsoredPresets && (
        <p
          className="text-xs text-muted-foreground"
          data-testid="sponsor-disclosure"
        >
          {t("providerPreset.sponsorDisclosure", {
            defaultValue:
              "Presets marked with a star or heart are sponsored placements; their sign-up links may earn CC Switch a referral commission.",
          })}{" "}
          <button
            type="button"
            onClick={openSponsorPolicy}
            className="underline underline-offset-2 hover:text-foreground"
          >
            {t("providerPreset.sponsorPolicyLink", {
              defaultValue: "Sponsor policy",
            })}
          </button>
        </p>
      )}
    </div>
  );
}
