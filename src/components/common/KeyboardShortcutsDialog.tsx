import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { isMac } from "@/lib/platform";
import {
  formatShortcut,
  useRegisteredShortcuts,
  useShortcut,
} from "@/lib/shortcuts";

/**
 * "?" opens a list of the keyboard shortcuts currently registered through
 * the shortcut registry (FE-UI-38), so the list always matches what the
 * current page actually handles.
 */
export function KeyboardShortcutsDialog() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const shortcuts = useRegisteredShortcuts();
  const mac = isMac();

  useShortcut(
    {
      id: "shell.shortcuts",
      keys: "shift+?",
      label: t("shortcuts.show", { defaultValue: "Show keyboard shortcuts" }),
    },
    () => setOpen((value) => !value),
  );

  const rows = [...shortcuts].sort((a, b) => a.label.localeCompare(b.label));

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t("shortcuts.title", { defaultValue: "Keyboard shortcuts" })}
          </DialogTitle>
          <DialogDescription>
            {t("shortcuts.description", {
              defaultValue: "Shortcuts available on the current page.",
            })}
          </DialogDescription>
        </DialogHeader>
        <dl className="space-y-2 px-6 pb-6">
          {rows.map((shortcut) => (
            <div
              key={shortcut.id}
              className="flex items-center justify-between gap-4 text-sm"
            >
              <dt>{shortcut.label}</dt>
              <dd>
                <kbd className="rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs">
                  {formatShortcut(shortcut.keys, mac)}
                </kbd>
              </dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  );
}
