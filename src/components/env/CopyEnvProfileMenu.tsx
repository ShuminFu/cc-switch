import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { renderProviderEnvProfile } from "@/lib/api/env";
import { copyText } from "@/lib/clipboard";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { EnvShell } from "@/types/env";

/** Apps whose providers `render_provider_env_profile` can export (SVC-35). */
export const ENV_PROFILE_APPS: readonly string[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
];

const SHELLS: { shell: EnvShell; label: string }[] = [
  { shell: "bash", label: "bash / zsh" },
  { shell: "fish", label: "fish" },
  { shell: "powershell", label: "PowerShell" },
  { shell: "dotenv", label: ".env" },
];

interface CopyEnvProfileMenuProps {
  appId: string;
  providerId: string;
  /** Render as an icon-only button (provider card actions). */
  iconOnly?: boolean;
}

/**
 * "Copy as shell profile": renders the provider's settings as environment
 * variables for bash/zsh, fish, PowerShell or a `.env` file and copies them.
 * The copied text contains the API key, so nothing is shown or logged.
 */
export function CopyEnvProfileMenu({
  appId,
  providerId,
  iconOnly = false,
}: CopyEnvProfileMenuProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  if (!ENV_PROFILE_APPS.includes(appId)) return null;

  const label = t("env.profile.copy", {
    defaultValue: "Copy as shell profile",
  });

  const copy = async (shell: EnvShell) => {
    setBusy(true);
    try {
      const text = await renderProviderEnvProfile(appId, providerId, shell);
      await copyText(text);
      toast.success(
        t("env.profile.copied", {
          defaultValue: "Environment variables copied (includes the API key)",
        }),
      );
    } catch (error) {
      toast.error(
        t("env.profile.copyFailed", {
          defaultValue: "Could not export environment variables",
        }),
        { description: extractErrorMessage(error) || String(error) },
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size={iconOnly ? "icon" : "sm"}
          disabled={busy}
          aria-label={label}
          title={label}
          className={iconOnly ? undefined : "gap-1.5"}
        >
          <Terminal className="h-4 w-4" />
          {!iconOnly && label}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>{label}</DropdownMenuLabel>
        {SHELLS.map(({ shell, label: shellLabel }) => (
          <DropdownMenuItem key={shell} onSelect={() => void copy(shell)}>
            {shellLabel}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
