import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { settingsApi, type CopilotOptimizerConfig } from "@/lib/api/settings";

/** Keep in sync with CopilotOptimizerConfig::default() in src-tauri/src/proxy/types.rs */
export const DEFAULT_COPILOT_OPTIMIZER_CONFIG: CopilotOptimizerConfig = {
  enabled: true,
  requestClassification: true,
  toolResultMerging: true,
  compactDetection: true,
  deterministicRequestId: true,
  subagentDetection: true,
  warmupDowngrade: true,
  warmupModel: "gpt-5-mini",
  stripThinking: true,
};

type ToggleKey = Exclude<
  keyof CopilotOptimizerConfig,
  "enabled" | "warmupModel"
>;

const TOGGLES: ToggleKey[] = [
  "requestClassification",
  "toolResultMerging",
  "compactDetection",
  "deterministicRequestId",
  "subagentDetection",
  "warmupDowngrade",
  "stripThinking",
];

/**
 * GitHub Copilot 优化器设置
 *
 * 后端自 v3.11 起持久化并执行这些开关（settings.copilot_optimizer_config），
 * 但此前没有任何界面入口；遇到 premium 配额异常消耗的用户只能改数据库。
 */
export function CopilotOptimizerPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<CopilotOptimizerConfig>(
    DEFAULT_COPILOT_OPTIMIZER_CONFIG,
  );
  const [warmupModelDraft, setWarmupModelDraft] = useState(
    DEFAULT_COPILOT_OPTIMIZER_CONFIG.warmupModel,
  );
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    settingsApi
      .getCopilotOptimizerConfig()
      .then((loaded) => {
        setConfig(loaded);
        setWarmupModelDraft(loaded.warmupModel);
      })
      .catch((e) =>
        console.error("Failed to load Copilot optimizer config:", e),
      )
      .finally(() => setIsLoading(false));
  }, []);

  const save = async (updates: Partial<CopilotOptimizerConfig>) => {
    const next = { ...config, ...updates };
    setConfig(next);
    try {
      await settingsApi.setCopilotOptimizerConfig(next);
    } catch (e) {
      console.error("Failed to save Copilot optimizer config:", e);
      toast.error(String(e));
      setConfig(config);
      setWarmupModelDraft(config.warmupModel);
    }
  };

  const commitWarmupModel = () => {
    const trimmed =
      warmupModelDraft.trim() || DEFAULT_COPILOT_OPTIMIZER_CONFIG.warmupModel;
    setWarmupModelDraft(trimmed);
    if (trimmed !== config.warmupModel) {
      void save({ warmupModel: trimmed });
    }
  };

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label htmlFor="copilot-optimizer-enabled">
            {t("settings.advanced.copilotOptimizer.enabled")}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.copilotOptimizer.enabledDescription")}
          </p>
        </div>
        <Switch
          id="copilot-optimizer-enabled"
          checked={config.enabled}
          onCheckedChange={(checked) => void save({ enabled: checked })}
        />
      </div>

      <div className="space-y-4 pl-4">
        {TOGGLES.map((key) => (
          <div key={key} className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor={`copilot-optimizer-${key}`}>
                {t(`settings.advanced.copilotOptimizer.${key}`)}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t(`settings.advanced.copilotOptimizer.${key}Description`)}
              </p>
            </div>
            <Switch
              id={`copilot-optimizer-${key}`}
              checked={config[key]}
              disabled={!config.enabled}
              onCheckedChange={(checked) => void save({ [key]: checked })}
            />
          </div>
        ))}

        <div className="flex items-center justify-between gap-4 pl-4">
          <div className="space-y-0.5">
            <Label htmlFor="copilot-optimizer-warmup-model">
              {t("settings.advanced.copilotOptimizer.warmupModel")}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.copilotOptimizer.warmupModelDescription")}
            </p>
          </div>
          <Input
            id="copilot-optimizer-warmup-model"
            className="w-48 shrink-0"
            value={warmupModelDraft}
            disabled={!config.enabled || !config.warmupDowngrade}
            placeholder={DEFAULT_COPILOT_OPTIMIZER_CONFIG.warmupModel}
            onChange={(event) => setWarmupModelDraft(event.target.value)}
            onBlur={commitWarmupModel}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitWarmupModel();
              }
            }}
          />
        </div>
      </div>
    </div>
  );
}
