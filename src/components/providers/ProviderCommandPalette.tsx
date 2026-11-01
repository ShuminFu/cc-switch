import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { APP_REGISTRY, ALL_APP_IDS, appHas, isAppVisible } from "@/config/apps";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useProviderActions } from "@/hooks/useProviderActions";
import {
  formatShortcut,
  useRegisteredShortcuts,
  useShortcut,
} from "@/lib/shortcuts";
import { isMac } from "@/lib/platform";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";

export interface PaletteNavigation {
  activeApp: AppId;
  setActiveApp: (app: AppId) => void;
  /** Open a page by view id (settings, sessions, mcp, ...). */
  openPage: (view: PalettePage) => void;
}

export type PalettePage =
  | "providers"
  | "settings"
  | "sessions"
  | "mcp"
  | "prompts"
  | "skills";

const PAGES: { id: PalettePage; labelKey: string; defaultLabel: string }[] = [
  {
    id: "providers",
    labelKey: "palette.pageProviders",
    defaultLabel: "Providers",
  },
  { id: "settings", labelKey: "settings.title", defaultLabel: "Settings" },
  {
    id: "sessions",
    labelKey: "sessionManager.title",
    defaultLabel: "Session Manager",
  },
  { id: "mcp", labelKey: "mcp.title", defaultLabel: "MCP Management" },
  { id: "prompts", labelKey: "prompts.manage", defaultLabel: "Prompts" },
  { id: "skills", labelKey: "skills.manage", defaultLabel: "Skills" },
];

/**
 * One app's providers. A component per app so each gets its own
 * `useProviderActions(appId)`: selecting an entry switches the provider of
 * that app (FE-PROV-35), with the same proxy guards as the provider page.
 */
function AppProviderGroup({
  appId,
  onDone,
}: {
  appId: AppId;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const { data } = useProvidersQuery(appId);
  const { isRunning, takeoverStatus } = useProxyStatus();
  const takeover =
    (takeoverStatus as Partial<Record<AppId, boolean>> | undefined)?.[appId] ??
    false;
  const { switchProvider } = useProviderActions(
    appId,
    isRunning,
    isRunning && takeover,
  );
  const providers = useMemo<Provider[]>(
    () => Object.values(data?.providers ?? {}),
    [data?.providers],
  );
  if (providers.length === 0) return null;
  const definition = APP_REGISTRY[appId];
  const appLabel = t(definition.labelKey, { defaultValue: definition.label });
  // Additive apps (OpenCode / OpenClaw / Hermes) have no single current
  // provider to switch to; their entries only appear for the active app.
  const isAdditive = appHas(appId, "additiveLiveConfig");

  return (
    <CommandGroup heading={appLabel}>
      {providers.map((provider) => {
        const isCurrent =
          !isAdditive && provider.id === data?.currentProviderId;
        return (
          <CommandItem
            key={`${appId}:${provider.id}`}
            value={`${appLabel} ${provider.name} ${provider.id}`}
            onSelect={() => {
              onDone();
              if (!isCurrent) void switchProvider(provider);
            }}
          >
            <span className="flex-1 truncate">{provider.name}</span>
            {isCurrent && <Check className="h-4 w-4 text-primary" />}
          </CommandItem>
        );
      })}
    </CommandGroup>
  );
}

/**
 * Cmd/Ctrl+K quick switcher (FE-ARCH-16, FE-PROV-35): switch provider in any
 * visible app, switch app, open a page, and see the registered shortcuts.
 */
export function ProviderCommandPalette({
  navigation,
}: {
  navigation: PaletteNavigation;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const { data: settings } = useSettingsQuery();
  const shortcuts = useRegisteredShortcuts();
  const mac = isMac();

  useShortcut(
    {
      id: "palette.open",
      keys: "mod+k",
      label: t("palette.shortcut", {
        defaultValue: "Open the command palette",
      }),
      allowInInput: true,
    },
    () => setOpen((value) => !value),
  );

  const visibleApps = ALL_APP_IDS.filter((id) =>
    isAppVisible(settings?.visibleApps, id),
  );
  // The active app first; non-additive apps can be switched from anywhere.
  const providerApps = [
    navigation.activeApp,
    ...visibleApps.filter(
      (id) => id !== navigation.activeApp && !appHas(id, "additiveLiveConfig"),
    ),
  ];
  const close = () => setOpen(false);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="p-0 overflow-hidden max-w-lg">
        <DialogTitle className="sr-only">
          {t("palette.title", { defaultValue: "Command palette" })}
        </DialogTitle>
        <Command>
          <CommandInput
            placeholder={t("palette.placeholder", {
              defaultValue: "Switch provider, app or page...",
            })}
          />
          <CommandList className="max-h-[60vh]">
            <CommandEmpty>
              {t("palette.empty", { defaultValue: "No matches" })}
            </CommandEmpty>
            {open &&
              providerApps.map((appId) => (
                <AppProviderGroup key={appId} appId={appId} onDone={close} />
              ))}
            <CommandGroup
              heading={t("palette.apps", { defaultValue: "Switch app" })}
            >
              {visibleApps.map((appId) => {
                const definition = APP_REGISTRY[appId];
                return (
                  <CommandItem
                    key={`app:${appId}`}
                    value={`app ${definition.switcherLabel}`}
                    onSelect={() => {
                      close();
                      navigation.setActiveApp(appId);
                      navigation.openPage("providers");
                    }}
                  >
                    {definition.switcherLabel}
                  </CommandItem>
                );
              })}
            </CommandGroup>
            <CommandGroup
              heading={t("palette.pages", { defaultValue: "Open page" })}
            >
              {PAGES.map((page) => (
                <CommandItem
                  key={`page:${page.id}`}
                  value={`page ${t(page.labelKey, { defaultValue: page.defaultLabel })}`}
                  onSelect={() => {
                    close();
                    navigation.openPage(page.id);
                  }}
                >
                  {t(page.labelKey, { defaultValue: page.defaultLabel })}
                </CommandItem>
              ))}
            </CommandGroup>
            {shortcuts.length > 0 && (
              <CommandGroup
                heading={t("palette.shortcuts", {
                  defaultValue: "Keyboard shortcuts",
                })}
              >
                {shortcuts.map((shortcut) => (
                  <CommandItem
                    key={`shortcut:${shortcut.id}`}
                    value={`shortcut ${shortcut.label}`}
                    disabled
                  >
                    <span className="flex-1">{shortcut.label}</span>
                    <kbd className="text-xs text-muted-foreground">
                      {formatShortcut(shortcut.keys, mac)}
                    </kbd>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
