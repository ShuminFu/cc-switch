import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { settingsApi, type LogConfig } from "@/lib/api/settings";
import { queryKeys } from "@/lib/query/keys";
import { useBackendConfig } from "./useBackendConfig";
import { ConfigLoadErrorAlert } from "./ConfigLoadErrorAlert";

const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

export function LogConfigPanel() {
  const { t } = useTranslation();
  const {
    config,
    isLoading,
    error,
    refetch,
    update: handleChange,
  } = useBackendConfig<LogConfig>(
    queryKeys.backendConfig.log,
    () => settingsApi.getLogConfig(),
    (next) => settingsApi.setLogConfig(next),
    (e) => {
      console.error("Failed to save log config:", e);
      toast.error(String(e));
    },
  );

  if (isLoading) return null;
  if (!config) {
    return <ConfigLoadErrorAlert error={error} onRetry={() => refetch()} />;
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.logConfig.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.logConfig.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.logConfig.level")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.logConfig.levelDescription")}
          </p>
        </div>
        <Select
          value={config.level}
          disabled={!config.enabled}
          onValueChange={(value) =>
            handleChange({ level: value as LogConfig["level"] })
          }
        >
          <SelectTrigger className="w-[120px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {LOG_LEVELS.map((level) => (
              <SelectItem key={level} value={level}>
                {t(`settings.advanced.logConfig.levels.${level}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* 日志级别说明 */}
      <div className="rounded-lg bg-muted/50 p-4 text-xs space-y-1.5">
        <p className="font-medium text-muted-foreground mb-2">
          {t("settings.advanced.logConfig.levelHint")}
        </p>
        <div className="grid gap-1 text-muted-foreground">
          <p>
            <span className="font-mono text-red-500">error</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.error")}
          </p>
          <p>
            <span className="font-mono text-orange-500">warn</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.warn")}
          </p>
          <p>
            <span className="font-mono text-blue-500">info</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.info")}
          </p>
          <p>
            <span className="font-mono text-green-500">debug</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.debug")}
          </p>
          <p>
            <span className="font-mono text-gray-500">trace</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.trace")}
          </p>
        </div>
      </div>
    </div>
  );
}
