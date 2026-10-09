import { useTranslation } from "react-i18next";
import { useState, useEffect } from "react";
import { ChevronDown, ChevronRight, Coins, Wallet } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
export type PricingModelSourceOption = "inherit" | "request" | "response";

interface ProviderPricingConfig {
  enabled: boolean;
  costMultiplier?: string;
  pricingModelSource: PricingModelSourceOption;
}

export interface ProviderSpendLimits {
  /** 每日支出上限（USD），空字符串/undefined 表示不限 */
  dailyUsd?: string;
  /** 每月支出上限（USD） */
  monthlyUsd?: string;
}

interface ProviderAdvancedConfigProps {
  pricingConfig: ProviderPricingConfig;
  onPricingConfigChange: (config: ProviderPricingConfig) => void;
  spendLimits?: ProviderSpendLimits;
  onSpendLimitsChange?: (limits: ProviderSpendLimits) => void;
}

export function ProviderAdvancedConfig({
  pricingConfig,
  onPricingConfigChange,
  spendLimits,
  onSpendLimitsChange,
}: ProviderAdvancedConfigProps) {
  const { t } = useTranslation();
  const [isPricingConfigOpen, setIsPricingConfigOpen] = useState(
    pricingConfig.enabled,
  );
  const hasSpendLimits = Boolean(
    spendLimits?.dailyUsd?.trim() || spendLimits?.monthlyUsd?.trim(),
  );
  const [isSpendLimitsOpen, setIsSpendLimitsOpen] = useState(hasSpendLimits);

  useEffect(() => {
    setIsPricingConfigOpen(pricingConfig.enabled);
  }, [pricingConfig.enabled]);

  return (
    <div className="space-y-4">
      {/* 计费配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsPricingConfigOpen(!isPricingConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <Coins className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.pricingConfig", {
                defaultValue: "计费配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="pricing-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomPricing", {
                  defaultValue: "使用单独配置",
                })}
              </Label>
              <Switch
                id="pricing-config-enabled"
                checked={pricingConfig.enabled}
                onCheckedChange={(checked) => {
                  onPricingConfigChange({ ...pricingConfig, enabled: checked });
                  if (checked) setIsPricingConfigOpen(true);
                }}
              />
            </div>
            {isPricingConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isPricingConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.pricingConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的计费参数，不启用时使用全局默认配置。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="cost-multiplier">
                  {t("providerAdvanced.costMultiplier", {
                    defaultValue: "成本倍率",
                  })}
                </Label>
                <Input
                  id="cost-multiplier"
                  type="number"
                  step="0.01"
                  min="0"
                  inputMode="decimal"
                  value={pricingConfig.costMultiplier || ""}
                  onChange={(e) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      costMultiplier: e.target.value || undefined,
                    })
                  }
                  placeholder={t("providerAdvanced.costMultiplierPlaceholder", {
                    defaultValue: "留空使用全局默认（1）",
                  })}
                  disabled={!pricingConfig.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.costMultiplierHint", {
                    defaultValue: "实际成本 = 基础成本 × 倍率，支持小数如 1.5",
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="pricing-model-source">
                  {t("providerAdvanced.pricingModelSourceLabel", {
                    defaultValue: "计费模式",
                  })}
                </Label>
                <Select
                  value={pricingConfig.pricingModelSource}
                  onValueChange={(value) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      pricingModelSource: value as PricingModelSourceOption,
                    })
                  }
                  disabled={!pricingConfig.enabled}
                >
                  <SelectTrigger id="pricing-model-source">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="inherit">
                      {t("providerAdvanced.pricingModelSourceInherit", {
                        defaultValue: "继承全局默认",
                      })}
                    </SelectItem>
                    <SelectItem value="request">
                      {t("providerAdvanced.pricingModelSourceRequest", {
                        defaultValue: "请求模型",
                      })}
                    </SelectItem>
                    <SelectItem value="response">
                      {t("providerAdvanced.pricingModelSourceResponse", {
                        defaultValue: "返回模型",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.pricingModelSourceHint", {
                    defaultValue: "选择按请求模型还是返回模型进行定价匹配",
                  })}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 支出上限（后端 check_provider_limits 消费 meta.limitDailyUsd / limitMonthlyUsd） */}
      {onSpendLimitsChange && (
        <div className="rounded-lg border border-border/50 bg-muted/20">
          <button
            type="button"
            className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
            onClick={() => setIsSpendLimitsOpen(!isSpendLimitsOpen)}
          >
            <div className="flex items-center gap-3">
              <Wallet className="h-4 w-4 text-muted-foreground" />
              <span className="font-medium">
                {t("providerAdvanced.spendLimits", {
                  defaultValue: "支出上限",
                })}
              </span>
              {hasSpendLimits && (
                <span className="rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">
                  {t("providerAdvanced.spendLimitsActive", {
                    defaultValue: "已设置",
                  })}
                </span>
              )}
            </div>
            {isSpendLimitsOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </button>
          <div
            className={cn(
              "overflow-hidden transition-all duration-200",
              isSpendLimitsOpen
                ? "max-h-[500px] opacity-100"
                : "max-h-0 opacity-0",
            )}
          >
            <div className="border-t border-border/50 p-4 space-y-4">
              <p className="text-sm text-muted-foreground">
                {t("providerAdvanced.spendLimitsDesc", {
                  defaultValue:
                    "按代理记录的费用统计今日 / 本月支出，超过上限时在供应商卡片上提示；不会拦截请求。留空表示不限。",
                })}
              </p>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label htmlFor="spend-limit-daily">
                    {t("providerAdvanced.dailyLimit", {
                      defaultValue: "每日上限（USD）",
                    })}
                  </Label>
                  <Input
                    id="spend-limit-daily"
                    type="number"
                    step="0.01"
                    min="0"
                    inputMode="decimal"
                    value={spendLimits?.dailyUsd ?? ""}
                    onChange={(e) =>
                      onSpendLimitsChange({
                        ...spendLimits,
                        dailyUsd: e.target.value || undefined,
                      })
                    }
                    placeholder={t("providerAdvanced.limitPlaceholder", {
                      defaultValue: "不限",
                    })}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="spend-limit-monthly">
                    {t("providerAdvanced.monthlyLimit", {
                      defaultValue: "每月上限（USD）",
                    })}
                  </Label>
                  <Input
                    id="spend-limit-monthly"
                    type="number"
                    step="0.01"
                    min="0"
                    inputMode="decimal"
                    value={spendLimits?.monthlyUsd ?? ""}
                    onChange={(e) =>
                      onSpendLimitsChange({
                        ...spendLimits,
                        monthlyUsd: e.target.value || undefined,
                      })
                    }
                    placeholder={t("providerAdvanced.limitPlaceholder", {
                      defaultValue: "不限",
                    })}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
