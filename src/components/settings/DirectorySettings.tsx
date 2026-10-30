import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { ResolvedDirectories } from "@/hooks/useSettings";
import {
  DIRECTORY_APP_IDS,
  type DirectoryAppId,
} from "@/hooks/settingsDirectoryFields";

// i18n keys per app (kept literal so the key-usage check can see them).
const DIRECTORY_LABELS: Record<
  DirectoryAppId,
  { label: string; placeholder: string }
> = {
  claude: {
    label: "settings.claudeConfigDir",
    placeholder: "settings.browsePlaceholderClaude",
  },
  codex: {
    label: "settings.codexConfigDir",
    placeholder: "settings.browsePlaceholderCodex",
  },
  gemini: {
    label: "settings.geminiConfigDir",
    placeholder: "settings.browsePlaceholderGemini",
  },
  grokbuild: {
    label: "settings.grokConfigDir",
    placeholder: "settings.browsePlaceholderGrok",
  },
  opencode: {
    label: "settings.opencodeConfigDir",
    placeholder: "settings.browsePlaceholderOpencode",
  },
  openclaw: {
    label: "settings.openclawConfigDir",
    placeholder: "settings.browsePlaceholderOpenclaw",
  },
  hermes: {
    label: "settings.hermesConfigDir",
    placeholder: "settings.browsePlaceholderHermes",
  },
};

// Directory inputs keep the raw text while typing (so paths containing spaces
// can be typed) and are only trimmed when the field loses focus. An empty
// field means "no override": the resolved default is shown as placeholder.
function commitTrimmed(
  raw: string,
  current: string | undefined,
  onChange: (value?: string) => void,
) {
  const trimmed = raw.trim();
  const next = trimmed.length > 0 ? trimmed : undefined;
  if (next !== current) onChange(next);
}

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  /** Per-app override drafts (raw text), e.g. from `directoryValuesByApp`. */
  directories: Partial<Record<DirectoryAppId, string | undefined>>;
  onDirectoryChange: (app: DirectoryAppId, value?: string) => void;
  onBrowseDirectory: (app: DirectoryAppId) => Promise<void>;
  onResetDirectory: (app: DirectoryAppId) => Promise<void>;
}

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  onAppConfigChange,
  onBrowseAppConfig,
  onResetAppConfig,
  directories,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      {/* CC Switch 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">{t("settings.appConfigDir")}</h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.appConfigDirDescription")}
          </p>
        </header>

        <div className="flex items-center gap-2">
          <Input
            aria-label={t("settings.appConfigDir")}
            value={appConfigDir ?? ""}
            placeholder={
              resolvedDirs.appConfig || t("settings.browsePlaceholderApp")
            }
            className="text-xs"
            onChange={(event) => onAppConfigChange(event.target.value)}
            onBlur={(event) =>
              commitTrimmed(event.target.value, appConfigDir, onAppConfigChange)
            }
          />
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onBrowseAppConfig}
            title={t("settings.browseDirectory")}
          >
            <FolderSearch className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onResetAppConfig}
            title={t("settings.resetDefault")}
          >
            <Undo2 className="h-4 w-4" />
          </Button>
        </div>
      </section>

      {/* Claude/Codex 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">
            {t("settings.configDirectoryOverride")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.configDirectoryDescription")}
          </p>
        </header>

        {DIRECTORY_APP_IDS.map((app) => (
          <DirectoryInput
            key={app}
            label={t(DIRECTORY_LABELS[app].label)}
            value={directories[app]}
            resolvedValue={resolvedDirs[app]}
            placeholder={t(DIRECTORY_LABELS[app].placeholder)}
            onChange={(val) => onDirectoryChange(app, val)}
            onBrowse={() => onBrowseDirectory(app)}
            onReset={() => onResetDirectory(app)}
          />
        ))}
      </section>
    </div>
  );
}

interface DirectoryInputProps {
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
  onReset: () => Promise<void>;
}

function DirectoryInput({
  label,
  description,
  value,
  resolvedValue,
  placeholder,
  onChange,
  onBrowse,
  onReset,
}: DirectoryInputProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-1.5">
      <div className="space-y-1">
        <p className="text-xs font-medium text-foreground">{label}</p>
        {description ? (
          <p className="text-xs text-muted-foreground">{description}</p>
        ) : null}
      </div>
      <div className="flex items-center gap-2">
        <Input
          aria-label={label}
          value={value ?? ""}
          placeholder={resolvedValue || placeholder}
          className="text-xs"
          onChange={(event) => onChange(event.target.value)}
          onBlur={(event) => commitTrimmed(event.target.value, value, onChange)}
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={onBrowse}
          title={t("settings.browseDirectory")}
        >
          <FolderSearch className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={onReset}
          title={t("settings.resetDefault")}
        >
          <Undo2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
