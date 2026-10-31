import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { ALL_APP_IDS, APP_REGISTRY, isAppVisible } from "@/config/apps";
import { useNavigation, type ViewId } from "@/app/navigation";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useProxyStatus } from "@/hooks/useProxyStatus";
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

/** Whether a keyboard event is the palette shortcut (Cmd/Ctrl+K). */
export const isPaletteShortcut = (event: KeyboardEvent) =>
  (event.metaKey || event.ctrlKey) &&
  !event.altKey &&
  !event.shiftKey &&
  event.key.toLowerCase() === "k";

interface PageEntry {
  id: string;
  view: ViewId;
  tab?: string;
  label: string;
}

/**
 * Cmd/Ctrl+K command palette (PRODUCT-19): switch the active app's provider,
 * jump to another app, or open a page or settings tab without the mouse.
 */
export function CommandPalette() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPaletteShortcut(event)) return;
      event.preventDefault();
      setOpen((v) => !v);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-lg overflow-hidden p-0" zIndex="top">
        <DialogTitle className="sr-only">
          {t("palette.title", { defaultValue: "Command palette" })}
        </DialogTitle>
        <DialogDescription className="sr-only">
          {t("palette.description", {
            defaultValue: "Search for an action and press Enter",
          })}
        </DialogDescription>
        {open && <PaletteBody onDone={() => setOpen(false)} />}
      </DialogContent>
    </Dialog>
  );
}

function PaletteBody({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const { activeApp, setActiveApp, navigate } = useNavigation();
  const { data: settings } = useSettingsQuery();
  const { isRunning: isProxyRunning, takeoverStatus } = useProxyStatus();
  const isTakeover =
    isProxyRunning &&
    Boolean(
      (takeoverStatus as Partial<Record<AppId, boolean>> | undefined)?.[
        activeApp
      ],
    );
  const { data } = useProvidersQuery(activeApp, { isProxyRunning });
  const { switchProvider } = useProviderActions(
    activeApp,
    isProxyRunning,
    isTakeover,
  );

  const providers = useMemo(
    () =>
      Object.values(data?.providers ?? {}).sort(
        (a, b) => (a.sortIndex ?? 0) - (b.sortIndex ?? 0),
      ),
    [data],
  );
  const currentId = data?.currentProviderId ?? "";
  const appLabel = (id: AppId) =>
    t(`apps.${id}`, { defaultValue: APP_REGISTRY[id].label });
  const visibleApps = ALL_APP_IDS.filter((id) =>
    isAppVisible(settings?.visibleApps, id),
  );

  const pages: PageEntry[] = [
    {
      id: "providers",
      view: "providers",
      label: t("palette.providers", { defaultValue: "Providers" }),
    },
    {
      id: "usage",
      view: "settings",
      tab: "usage",
      label: t("usage.title"),
    },
    {
      id: "proxy",
      view: "settings",
      tab: "proxy",
      label: t("settings.tabProxy"),
    },
    { id: "mcp", view: "mcp", label: t("mcp.unifiedPanel.title") },
    { id: "skills", view: "skills", label: t("skills.title") },
    {
      id: "prompts",
      view: "prompts",
      label: t("prompts.manage", { defaultValue: "Prompts" }),
    },
    { id: "sessions", view: "sessions", label: t("sessionManager.title") },
    {
      id: "universal",
      view: "universal",
      label: t("universalProvider.title", { defaultValue: "统一供应商" }),
    },
    { id: "settings", view: "settings", label: t("settings.title") },
  ];

  const run = (action: () => unknown) => {
    onDone();
    void action();
  };

  return (
    <Command loop>
      <CommandInput
        autoFocus
        placeholder={t("palette.placeholder", {
          defaultValue: "Switch provider, open a page…",
        })}
      />
      <CommandList className="max-h-[360px]">
        <CommandEmpty>
          {t("palette.empty", { defaultValue: "No matching action" })}
        </CommandEmpty>
        {providers.length > 0 && (
          <CommandGroup
            heading={t("palette.switchProvider", {
              defaultValue: "Switch {{app}} provider",
              app: appLabel(activeApp),
            })}
          >
            {providers.map((provider: Provider) => (
              <CommandItem
                key={provider.id}
                value={`provider ${provider.name} ${provider.id}`}
                disabled={provider.id === currentId}
                onSelect={() => run(() => switchProvider(provider))}
              >
                <span className="flex-1 truncate">{provider.name}</span>
                {provider.id === currentId && (
                  <Check
                    className="h-4 w-4 opacity-70"
                    aria-label={t("palette.current", {
                      defaultValue: "Current",
                    })}
                  />
                )}
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        <CommandGroup
          heading={t("palette.apps", { defaultValue: "Go to app" })}
        >
          {visibleApps
            .filter((id) => id !== activeApp)
            .map((id) => (
              <CommandItem
                key={id}
                value={`app ${APP_REGISTRY[id].label} ${id}`}
                onSelect={() =>
                  run(() => {
                    setActiveApp(id);
                    navigate("providers");
                  })
                }
              >
                {appLabel(id)}
              </CommandItem>
            ))}
        </CommandGroup>
        <CommandGroup
          heading={t("palette.pages", { defaultValue: "Open page" })}
        >
          {pages.map((page) => (
            <CommandItem
              key={page.id}
              value={`page ${page.label} ${page.id}`}
              onSelect={() =>
                run(() =>
                  page.view === "settings"
                    ? navigate(
                        "settings",
                        page.tab ? { tab: page.tab } : undefined,
                      )
                    : navigate(page.view),
                )
              }
            >
              {page.label}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </Command>
  );
}
