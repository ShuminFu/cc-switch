import { useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { useShortcut } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import {
  searchSettings,
  settingsEntryTitle,
  type SettingsSearchEntry,
} from "./settingsSearchIndex";

interface SettingsSearchProps {
  onSelect: (entry: SettingsSearchEntry) => void;
}

const TAB_LABEL_KEYS: Record<SettingsSearchEntry["tab"], [string, string]> = {
  general: ["settings.tabGeneral", "General"],
  proxy: ["settings.tabProxy", "Proxy"],
  auth: ["settings.tabAuth", "Authentication"],
  advanced: ["settings.tabAdvanced", "Advanced"],
  usage: ["usage.title", "Usage"],
  about: ["common.about", "About"],
};

/**
 * Settings search box (FE-UI-38): type to find a setting, Enter or click to
 * jump to it. Cmd/Ctrl+F focuses the box while Settings is open.
 */
export function SettingsSearch({ onSelect }: SettingsSearchProps) {
  const { t } = useTranslation();
  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const results = useMemo(() => searchSettings(query, t), [query, t]);

  useShortcut(
    {
      id: "settings.search",
      keys: "mod+f",
      label: t("settings.search.shortcut", {
        defaultValue: "Search settings",
      }),
      allowInInput: true,
    },
    () => {
      inputRef.current?.focus();
      inputRef.current?.select();
    },
  );

  const choose = (entry: SettingsSearchEntry) => {
    setQuery("");
    setActive(0);
    onSelect(entry);
  };

  const open = query.trim().length > 0;

  return (
    <div className="relative mb-4">
      <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        ref={inputRef}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setActive((i) => Math.min(i + 1, Math.max(results.length - 1, 0)));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setActive((i) => Math.max(i - 1, 0));
          } else if (e.key === "Enter" && results[active]) {
            e.preventDefault();
            choose(results[active]);
          } else if (e.key === "Escape" && query) {
            e.preventDefault();
            e.stopPropagation();
            setQuery("");
          }
        }}
        placeholder={t("settings.search.placeholder", {
          defaultValue: "Search settings…",
        })}
        aria-label={t("settings.search.label", {
          defaultValue: "Search settings",
        })}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        className="pl-9"
      />
      {open && (
        <ul
          id={listId}
          role="listbox"
          className="absolute z-50 mt-1 max-h-72 w-full overflow-y-auto rounded-md border border-border bg-popover p-1 shadow-md"
        >
          {results.length === 0 ? (
            <li className="px-3 py-2 text-sm text-muted-foreground">
              {t("settings.search.noResults", {
                defaultValue: "No matching settings",
              })}
            </li>
          ) : (
            results.map((entry, index) => {
              const [tabKey, tabDefault] = TAB_LABEL_KEYS[entry.tab];
              return (
                <li
                  key={entry.id}
                  role="option"
                  aria-selected={index === active}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => choose(entry)}
                  className={cn(
                    "flex cursor-pointer items-center justify-between gap-3 rounded-sm px-3 py-2 text-sm",
                    index === active && "bg-muted",
                  )}
                >
                  <span>{settingsEntryTitle(entry, t)}</span>
                  <span className="text-xs text-muted-foreground">
                    {t(tabKey, { defaultValue: tabDefault })}
                  </span>
                </li>
              );
            })
          )}
        </ul>
      )}
    </div>
  );
}
