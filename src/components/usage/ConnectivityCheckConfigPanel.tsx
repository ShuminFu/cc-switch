import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2, Info } from "lucide-react";
import { toast } from "sonner";
import {
  getStreamCheckConfig,
  saveStreamCheckConfig,
  type StreamCheckConfig,
} from "@/lib/api/connectivity-check";
import { queryKeys } from "@/lib/query/keys";
import { useBackendConfig } from "@/components/settings/useBackendConfig";
import { ConfigLoadErrorAlert } from "@/components/settings/ConfigLoadErrorAlert";
import {
  STREAM_CHECK_LIMITS,
  parseStreamCheckDraft,
  toStreamCheckDraft,
  type StreamCheckDraft,
  type StreamCheckField,
} from "./streamCheckDraft";

export function ConnectivityCheckConfigPanel() {
  const { t } = useTranslation();
  const {
    config: saved,
    isLoading,
    error,
    refetch,
    isSaving,
    saveAsync,
  } = useBackendConfig<StreamCheckConfig>(
    queryKeys.backendConfig.streamCheck,
    getStreamCheckConfig,
    saveStreamCheckConfig,
  );
  // String drafts so a number input can be cleared while typing.
  const [config, setConfig] = useState<StreamCheckDraft | null>(null);
  const [showErrors, setShowErrors] = useState(false);

  useEffect(() => {
    if (saved && config === null) setConfig(toStreamCheckDraft(saved));
  }, [saved, config]);

  const { config: parsedConfig, invalid } = config
    ? parseStreamCheckDraft(config)
    : { config: null, invalid: [] as StreamCheckField[] };

  async function handleSave() {
    if (!parsedConfig) {
      setShowErrors(true);
      return;
    }
    try {
      await saveAsync(parsedConfig);
      setShowErrors(false);
      toast.success(t("streamCheck.configSaved"), {
        closeButton: true,
      });
    } catch (e) {
      toast.error(t("streamCheck.configSaveFailed") + ": " + String(e));
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!saved || !config) {
    return <ConfigLoadErrorAlert error={error} onRetry={() => refetch()} />;
  }

  const fieldError = (key: StreamCheckField) =>
    showErrors && invalid.includes(key) ? (
      <p className="text-xs text-destructive" role="alert">
        {t("streamCheck.rangeError", {
          defaultValue: "Enter a whole number between {{min}} and {{max}}.",
          ...STREAM_CHECK_LIMITS[key],
        })}
      </p>
    ) : null;

  return (
    <div className="space-y-6">
      {/* 连通检测语义说明：可达 ≠ 配置正确 */}
      <Alert>
        <Info className="h-4 w-4" />
        <AlertDescription>
          {t("streamCheck.connectivityNote", {
            defaultValue:
              "连通检测仅探测供应商地址是否可达，不发送真实模型请求。收到任意响应即视为“可达”——这不代表鉴权或模型配置一定正确。",
          })}
        </AlertDescription>
      </Alert>

      {/* 检查参数配置 */}
      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("streamCheck.checkParams")}
        </h4>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-2">
            <Label htmlFor="timeoutSecs">{t("streamCheck.timeout")}</Label>
            <Input
              id="timeoutSecs"
              type="number"
              min={2}
              max={60}
              value={config.timeoutSecs}
              onChange={(e) =>
                setConfig({ ...config, timeoutSecs: e.target.value })
              }
              aria-invalid={showErrors && invalid.includes("timeoutSecs")}
            />
            {fieldError("timeoutSecs")}
          </div>

          <div className="space-y-2">
            <Label htmlFor="maxRetries">{t("streamCheck.maxRetries")}</Label>
            <Input
              id="maxRetries"
              type="number"
              min={0}
              max={5}
              value={config.maxRetries}
              onChange={(e) =>
                setConfig({ ...config, maxRetries: e.target.value })
              }
              aria-invalid={showErrors && invalid.includes("maxRetries")}
            />
            {fieldError("maxRetries")}
          </div>

          <div className="space-y-2">
            <Label htmlFor="degradedThresholdMs">
              {t("streamCheck.degradedThreshold")}
            </Label>
            <Input
              id="degradedThresholdMs"
              type="number"
              min={1000}
              max={30000}
              step={1000}
              value={config.degradedThresholdMs}
              onChange={(e) =>
                setConfig({ ...config, degradedThresholdMs: e.target.value })
              }
              aria-invalid={
                showErrors && invalid.includes("degradedThresholdMs")
              }
            />
            {fieldError("degradedThresholdMs")}
          </div>
        </div>
      </div>

      <div className="flex justify-end">
        <Button onClick={handleSave} disabled={isSaving}>
          {isSaving ? (
            <>
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              {t("common.saving")}
            </>
          ) : (
            <>
              <Save className="mr-2 h-4 w-4" />
              {t("common.save")}
            </>
          )}
        </Button>
      </div>
    </div>
  );
}
