import {
  useCircuitBreakerConfig,
  useUpdateCircuitBreakerConfig,
} from "@/lib/query/failover";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { useState, useEffect } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

/**
 * 熔断器配置面板
 * 允许用户调整熔断器参数
 */
export function CircuitBreakerConfigPanel() {
  const { t } = useTranslation();
  const { data: config, isLoading } = useCircuitBreakerConfig();
  const updateConfig = useUpdateCircuitBreakerConfig();

  // 使用字符串状态以支持完全清空输入框
  const [formData, setFormData] = useState({
    failureThreshold: "5",
    successThreshold: "2",
    timeoutSeconds: "60",
    errorRateThreshold: "50", // 存储百分比值
    minRequests: "10",
  });

  // 当配置加载完成时更新表单数据
  useEffect(() => {
    if (config) {
      setFormData({
        failureThreshold: String(config.failureThreshold),
        successThreshold: String(config.successThreshold),
        timeoutSeconds: String(config.timeoutSeconds),
        errorRateThreshold: String(Math.round(config.errorRateThreshold * 100)),
        minRequests: String(config.minRequests),
      });
    }
  }, [config]);

  const handleSave = async () => {
    // 解析数字，返回 NaN 表示无效输入
    const parseNum = (val: string) => {
      const trimmed = val.trim();
      // 必须是纯数字
      if (!/^-?\d+$/.test(trimmed)) return NaN;
      return parseInt(trimmed);
    };

    // 定义各字段的有效范围
    const ranges = {
      failureThreshold: { min: 1, max: 20 },
      successThreshold: { min: 1, max: 10 },
      timeoutSeconds: { min: 0, max: 300 },
      errorRateThreshold: { min: 0, max: 100 },
      minRequests: { min: 5, max: 100 },
    };

    // 解析原始值
    const raw = {
      failureThreshold: parseNum(formData.failureThreshold),
      successThreshold: parseNum(formData.successThreshold),
      timeoutSeconds: parseNum(formData.timeoutSeconds),
      errorRateThreshold: parseNum(formData.errorRateThreshold),
      minRequests: parseNum(formData.minRequests),
    };

    // 校验是否超出范围（NaN 也视为无效）
    const errors: string[] = [];
    const checkRange = (
      value: number,
      range: { min: number; max: number },
      label: string,
    ) => {
      if (isNaN(value) || value < range.min || value > range.max) {
        errors.push(`${label}: ${range.min}-${range.max}`);
      }
    };

    checkRange(
      raw.failureThreshold,
      ranges.failureThreshold,
      t("circuitBreaker.failureThreshold"),
    );
    checkRange(
      raw.successThreshold,
      ranges.successThreshold,
      t("circuitBreaker.successThreshold"),
    );
    checkRange(
      raw.timeoutSeconds,
      ranges.timeoutSeconds,
      t("circuitBreaker.timeoutSeconds"),
    );
    checkRange(
      raw.errorRateThreshold,
      ranges.errorRateThreshold,
      t("circuitBreaker.errorRateThreshold"),
    );
    checkRange(
      raw.minRequests,
      ranges.minRequests,
      t("circuitBreaker.minRequests"),
    );

    if (errors.length > 0) {
      toast.error(
        t("circuitBreaker.validationFailed", {
          fields: errors.join("; "),
        }),
      );
      return;
    }

    try {
      await updateConfig.mutateAsync({
        failureThreshold: raw.failureThreshold,
        successThreshold: raw.successThreshold,
        timeoutSeconds: raw.timeoutSeconds,
        errorRateThreshold: raw.errorRateThreshold / 100,
        minRequests: raw.minRequests,
      });
      toast.success(t("circuitBreaker.configSaved"), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("circuitBreaker.saveFailed") + ": " + String(error));
    }
  };

  const handleReset = () => {
    if (config) {
      setFormData({
        failureThreshold: String(config.failureThreshold),
        successThreshold: String(config.successThreshold),
        timeoutSeconds: String(config.timeoutSeconds),
        errorRateThreshold: String(Math.round(config.errorRateThreshold * 100)),
        minRequests: String(config.minRequests),
      });
    }
  };

  if (isLoading) {
    return (
      <div className="text-sm text-muted-foreground">
        {t("circuitBreaker.loading")}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold">{t("circuitBreaker.title")}</h3>
        <p className="text-sm text-muted-foreground mt-1">
          {t("circuitBreaker.description")}
        </p>
      </div>

      <div className="h-px bg-border my-4" />

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* 失败阈值 */}
        <div className="space-y-2">
          <Label htmlFor="failureThreshold">
            {t("circuitBreaker.failureThreshold")}
          </Label>
          <Input
            id="failureThreshold"
            type="number"
            min="1"
            max="20"
            value={formData.failureThreshold}
            onChange={(e) =>
              setFormData({ ...formData, failureThreshold: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">
            {t("circuitBreaker.failureThresholdHint")}
          </p>
        </div>

        {/* 超时时间 */}
        <div className="space-y-2">
          <Label htmlFor="timeoutSeconds">
            {t("circuitBreaker.timeoutSeconds")}
          </Label>
          <Input
            id="timeoutSeconds"
            type="number"
            min="0"
            max="300"
            value={formData.timeoutSeconds}
            onChange={(e) =>
              setFormData({ ...formData, timeoutSeconds: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">
            {t("circuitBreaker.timeoutSecondsHint")}
          </p>
        </div>

        {/* 成功阈值 */}
        <div className="space-y-2">
          <Label htmlFor="successThreshold">
            {t("circuitBreaker.successThreshold")}
          </Label>
          <Input
            id="successThreshold"
            type="number"
            min="1"
            max="10"
            value={formData.successThreshold}
            onChange={(e) =>
              setFormData({ ...formData, successThreshold: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">
            {t("circuitBreaker.successThresholdHint")}
          </p>
        </div>

        {/* 错误率阈值 */}
        <div className="space-y-2">
          <Label htmlFor="errorRateThreshold">
            {t("circuitBreaker.errorRateThreshold")}
          </Label>
          <Input
            id="errorRateThreshold"
            type="number"
            min="0"
            max="100"
            step="5"
            value={formData.errorRateThreshold}
            onChange={(e) =>
              setFormData({ ...formData, errorRateThreshold: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">
            {t("circuitBreaker.errorRateThresholdHint")}
          </p>
        </div>

        {/* 最小请求数 */}
        <div className="space-y-2">
          <Label htmlFor="minRequests">{t("circuitBreaker.minRequests")}</Label>
          <Input
            id="minRequests"
            type="number"
            min="5"
            max="100"
            value={formData.minRequests}
            onChange={(e) =>
              setFormData({ ...formData, minRequests: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">
            {t("circuitBreaker.minRequestsHint")}
          </p>
        </div>
      </div>

      <div className="flex gap-3">
        <Button onClick={handleSave} disabled={updateConfig.isPending}>
          {updateConfig.isPending
            ? t("common.saving")
            : t("circuitBreaker.saveConfig")}
        </Button>
        <Button
          variant="outline"
          onClick={handleReset}
          disabled={updateConfig.isPending}
        >
          {t("common.reset")}
        </Button>
      </div>

      {/* 说明信息 */}
      <div className="p-4 bg-muted/50 rounded-lg space-y-2 text-sm">
        <h4 className="font-medium">{t("circuitBreaker.instructionsTitle")}</h4>
        <ul className="space-y-1 text-muted-foreground">
          <li>
            • <strong>{t("circuitBreaker.failureThreshold")}</strong>：
            {t("circuitBreaker.instructions.failureThreshold")}
          </li>
          <li>
            • <strong>{t("circuitBreaker.timeoutSeconds")}</strong>：
            {t("circuitBreaker.instructions.timeout")}
          </li>
          <li>
            • <strong>{t("circuitBreaker.successThreshold")}</strong>：
            {t("circuitBreaker.instructions.successThreshold")}
          </li>
          <li>
            • <strong>{t("circuitBreaker.errorRateThreshold")}</strong>：
            {t("circuitBreaker.instructions.errorRate")}
          </li>
          <li>
            • <strong>{t("circuitBreaker.minRequests")}</strong>：
            {t("circuitBreaker.instructions.minRequests")}
          </li>
        </ul>
      </div>
    </div>
  );
}
