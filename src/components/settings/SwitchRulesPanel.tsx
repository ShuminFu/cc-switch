import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Loader2, Plus, RefreshCw, Trash2, Undo2, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
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
import { useProvidersQuery } from "@/lib/query/queries";
import {
  useCancelSwitchRuleRevert,
  useDeleteSwitchRule,
  useEvaluateSwitchRulesNow,
  useSetSwitchRuleEnabled,
  useSwitchRuleStates,
  useSwitchRules,
  useUpsertSwitchRule,
} from "@/lib/query/switchRules";
import type { SwitchRule, SwitchRuleSource } from "@/lib/api/switchRules";
import type { AppId } from "@/lib/api/types";
import { TIER_I18N_KEYS } from "@/components/SubscriptionQuotaFooter";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 支持规则的应用：有「当前供应商」概念且有官方订阅 / Coding Plan 额度 */
export const SWITCH_RULE_APPS: AppId[] = ["claude", "codex", "gemini"];

/** 各来源可选的窗口名；与后端 subscription.rs 的 TIER_* 常量一致 */
export const SUBSCRIPTION_TIERS: Record<string, string[]> = {
  claude: ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"],
  codex: ["five_hour", "seven_day", "30_day"],
  gemini: ["gemini_pro", "gemini_flash", "gemini_flash_lite"],
};
export const CODING_PLAN_TIERS = ["five_hour", "weekly_limit", "monthly"];

const OFFICIAL_SOURCE = "__official__";

interface SwitchRulesPanelProps {
  appType: AppId;
  disabled?: boolean;
}

interface DraftForm {
  watched: string;
  tierName: string;
  thresholdPct: string;
  targetProviderId: string;
  revertOnReset: boolean;
}

const emptyDraft = (): DraftForm => ({
  watched: OFFICIAL_SOURCE,
  tierName: "",
  thresholdPct: "90",
  targetProviderId: "",
  revertOnReset: true,
});

export function formatRuleTime(
  value: number | undefined,
  locale: string,
): string {
  if (!value) return "--";
  return new Date(value * 1000).toLocaleString(locale);
}

export function SwitchRulesPanel({
  appType,
  disabled = false,
}: SwitchRulesPanelProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || "en";
  const { data: rules = [], isLoading } = useSwitchRules(appType);
  const { data: states = [] } = useSwitchRuleStates();
  const { data: providersData } = useProvidersQuery(appType);
  const upsertRule = useUpsertSwitchRule();
  const deleteRule = useDeleteSwitchRule();
  const setEnabled = useSetSwitchRuleEnabled();
  const cancelRevert = useCancelSwitchRuleRevert();
  const evaluateNow = useEvaluateSwitchRulesNow();

  const [showEditor, setShowEditor] = useState(false);
  const [draft, setDraft] = useState<DraftForm>(emptyDraft);

  const providers = useMemo(
    () => Object.values(providersData?.providers ?? {}),
    [providersData],
  );
  const providerName = (id: string) =>
    providersData?.providers?.[id]?.name ?? id;
  /** 带 Coding Plan 用量脚本的供应商可以作为被监控的额度来源 */
  const codingPlanProviders = useMemo(
    () =>
      providers.filter(
        (p) => p.meta?.usage_script?.templateType === "token_plan",
      ),
    [providers],
  );
  const stateByRule = useMemo(
    () => new Map(states.map((s) => [s.ruleId, s])),
    [states],
  );

  const tierLabel = (name: string) =>
    TIER_I18N_KEYS[name] ? t(TIER_I18N_KEYS[name], name) : name;

  const source: SwitchRuleSource =
    draft.watched === OFFICIAL_SOURCE ? "subscription" : "coding_plan";
  const tierOptions =
    source === "subscription"
      ? (SUBSCRIPTION_TIERS[appType] ?? [])
      : CODING_PLAN_TIERS;
  const targetOptions = providers.filter(
    (p) =>
      p.id !== draft.watched &&
      !(source === "subscription" && p.category === "official"),
  );

  const resetEditor = () => {
    setDraft(emptyDraft());
    setShowEditor(false);
  };

  const handleSave = async () => {
    const threshold = Number(draft.thresholdPct);
    if (!Number.isFinite(threshold) || threshold < 1 || threshold > 100) {
      toast.error(
        t("switchRules.invalidThreshold", {
          defaultValue: "阈值需在 1 到 100 之间",
        }),
      );
      return;
    }
    if (!draft.tierName) {
      toast.error(
        t("switchRules.tierRequired", { defaultValue: "请选择额度窗口" }),
      );
      return;
    }
    if (!draft.targetProviderId) {
      toast.error(
        t("switchRules.targetRequired", {
          defaultValue: "请选择备用供应商",
        }),
      );
      return;
    }
    try {
      await upsertRule.mutateAsync({
        appType,
        source,
        watchedProviderId: source === "coding_plan" ? draft.watched : undefined,
        tierName: draft.tierName,
        thresholdPct: threshold,
        targetProviderId: draft.targetProviderId,
        revertOnReset: draft.revertOnReset,
        enabled: true,
      });
      toast.success(t("switchRules.saved", { defaultValue: "规则已保存" }));
      resetEditor();
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("switchRules.saveFailed", { defaultValue: "保存规则失败" }),
      );
    }
  };

  const handleEvaluateNow = async () => {
    try {
      const fired = await evaluateNow.mutateAsync(appType);
      toast.success(
        fired > 0
          ? t("switchRules.evaluatedFired", {
              defaultValue: "已评估，触发了 {{count}} 条规则",
              count: fired,
            })
          : t("switchRules.evaluatedIdle", {
              defaultValue: "已评估，暂无规则需要触发",
            }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("switchRules.evaluateFailed", { defaultValue: "评估失败" }),
      );
    }
  };

  const describeRule = (rule: SwitchRule) => {
    const watched =
      rule.source === "subscription"
        ? t("switchRules.officialPlan", { defaultValue: "官方订阅" })
        : providerName(rule.watchedProviderId ?? "");
    return t("switchRules.ruleSummary", {
      defaultValue:
        "{{watched}} 的 {{tier}} ≥ {{threshold}}% 时切换到 {{target}}",
      watched,
      tier: tierLabel(rule.tierName),
      threshold: rule.thresholdPct,
      target: providerName(rule.targetProviderId),
    });
  };

  return (
    <div className="space-y-4" data-testid={`switch-rules-${appType}`}>
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          {t("switchRules.hint", {
            defaultValue:
              "官方订阅规则在每次额度刷新时评估；Coding Plan 规则在该供应商的用量查询时评估。切换后会在窗口重置时自动回切（可关闭）。",
          })}
        </p>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="gap-1.5"
            disabled={disabled || evaluateNow.isPending || rules.length === 0}
            onClick={() => void handleEvaluateNow()}
          >
            {evaluateNow.isPending ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" />
            )}
            {t("switchRules.evaluateNow", { defaultValue: "立即评估" })}
          </Button>
          <Button
            type="button"
            size="sm"
            className="gap-1.5"
            disabled={disabled || showEditor}
            onClick={() => setShowEditor(true)}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("switchRules.add", { defaultValue: "添加规则" })}
          </Button>
        </div>
      </div>

      {showEditor && (
        <div
          className="space-y-4 rounded-lg border border-border/60 bg-muted/20 p-4"
          data-testid="switch-rule-editor"
        >
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor={`switch-rule-watched-${appType}`}>
                {t("switchRules.watched", { defaultValue: "监控的额度" })}
              </Label>
              <Select
                value={draft.watched}
                onValueChange={(value) =>
                  setDraft((d) => ({ ...d, watched: value, tierName: "" }))
                }
              >
                <SelectTrigger id={`switch-rule-watched-${appType}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={OFFICIAL_SOURCE}>
                    {t("switchRules.officialPlan", {
                      defaultValue: "官方订阅",
                    })}
                  </SelectItem>
                  {codingPlanProviders.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {t("switchRules.codingPlanOf", {
                        defaultValue: "{{name}}（Coding Plan）",
                        name: p.name,
                      })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor={`switch-rule-tier-${appType}`}>
                {t("switchRules.tier", { defaultValue: "额度窗口" })}
              </Label>
              <Select
                value={draft.tierName}
                onValueChange={(value) =>
                  setDraft((d) => ({ ...d, tierName: value }))
                }
              >
                <SelectTrigger id={`switch-rule-tier-${appType}`}>
                  <SelectValue
                    placeholder={t("switchRules.tierPlaceholder", {
                      defaultValue: "选择窗口",
                    })}
                  />
                </SelectTrigger>
                <SelectContent>
                  {tierOptions.map((tier) => (
                    <SelectItem key={tier} value={tier}>
                      {tierLabel(tier)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor={`switch-rule-threshold-${appType}`}>
                {t("switchRules.threshold", { defaultValue: "触发阈值（%）" })}
              </Label>
              <Input
                id={`switch-rule-threshold-${appType}`}
                type="number"
                min={1}
                max={100}
                step={1}
                inputMode="numeric"
                value={draft.thresholdPct}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, thresholdPct: e.target.value }))
                }
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor={`switch-rule-target-${appType}`}>
                {t("switchRules.target", { defaultValue: "切换到" })}
              </Label>
              <Select
                value={draft.targetProviderId}
                onValueChange={(value) =>
                  setDraft((d) => ({ ...d, targetProviderId: value }))
                }
              >
                <SelectTrigger id={`switch-rule-target-${appType}`}>
                  <SelectValue
                    placeholder={t("switchRules.targetPlaceholder", {
                      defaultValue: "选择备用供应商",
                    })}
                  />
                </SelectTrigger>
                <SelectContent>
                  {targetOptions.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-2">
              <Switch
                id={`switch-rule-revert-${appType}`}
                checked={draft.revertOnReset}
                onCheckedChange={(checked) =>
                  setDraft((d) => ({ ...d, revertOnReset: checked }))
                }
              />
              <Label
                htmlFor={`switch-rule-revert-${appType}`}
                className="text-sm text-muted-foreground"
              >
                {t("switchRules.revertOnReset", {
                  defaultValue: "窗口重置后自动切回",
                })}
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={resetEditor}
              >
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={upsertRule.isPending}
                onClick={() => void handleSave()}
              >
                {upsertRule.isPending && (
                  <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                )}
                {t("common.save", { defaultValue: "保存" })}
              </Button>
            </div>
          </div>
        </div>
      )}

      {isLoading ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("common.loading", { defaultValue: "加载中..." })}
        </div>
      ) : rules.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border/60 p-4 text-center text-xs text-muted-foreground">
          {t("switchRules.empty", {
            defaultValue:
              "还没有规则。添加一条，让额度用尽时自动切到备用供应商。",
          })}
        </p>
      ) : (
        <ul className="space-y-2">
          {rules.map((rule) => {
            const state = stateByRule.get(rule.id);
            return (
              <li
                key={rule.id}
                className="flex items-start justify-between gap-3 rounded-lg border border-border/60 bg-card/50 p-3"
                data-testid={`switch-rule-${rule.id}`}
              >
                <div className="min-w-0 space-y-1">
                  <div className="flex items-center gap-2">
                    <Zap className="h-3.5 w-3.5 shrink-0 text-amber-500" />
                    <span className="truncate text-sm font-medium">
                      {describeRule(rule)}
                    </span>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {rule.revertOnReset
                      ? t("switchRules.revertEnabled", {
                          defaultValue: "窗口重置后自动切回",
                        })
                      : t("switchRules.revertDisabled", {
                          defaultValue: "不自动切回",
                        })}
                    {rule.lastFiredAt
                      ? ` · ${t("switchRules.lastFired", {
                          defaultValue: "上次触发 {{time}}",
                          time: formatRuleTime(rule.lastFiredAt, locale),
                        })}`
                      : ""}
                  </p>
                  {state && (
                    <div
                      className="flex flex-wrap items-center gap-2 text-xs text-amber-600 dark:text-amber-400"
                      data-testid={`switch-rule-state-${rule.id}`}
                    >
                      <span>
                        {t("switchRules.armed", {
                          defaultValue:
                            "已触发（{{reason}}），{{from}} → {{to}}，{{revert}}",
                          reason: state.reason ?? "",
                          from: providerName(state.switchedFrom),
                          to: providerName(state.switchedTo),
                          revert: state.resetsAt
                            ? t("switchRules.revertAt", {
                                defaultValue: "将于 {{time}} 切回",
                                time: formatRuleTime(state.resetsAt, locale),
                              })
                            : t("switchRules.revertPending", {
                                defaultValue: "等待切回",
                              }),
                        })}
                      </span>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-6 gap-1 px-2 text-xs"
                        disabled={cancelRevert.isPending}
                        onClick={() => void cancelRevert.mutateAsync(rule.id)}
                      >
                        <Undo2 className="h-3 w-3" />
                        {t("switchRules.keepCurrent", {
                          defaultValue: "保持当前",
                        })}
                      </Button>
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Switch
                    checked={rule.enabled}
                    disabled={disabled || setEnabled.isPending}
                    aria-label={t("switchRules.enabled", {
                      defaultValue: "启用规则",
                    })}
                    onCheckedChange={(checked) =>
                      void setEnabled.mutateAsync({
                        id: rule.id,
                        enabled: checked,
                      })
                    }
                  />
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-8 w-8 p-0 text-muted-foreground hover:text-destructive"
                    aria-label={t("switchRules.delete", {
                      defaultValue: "删除规则",
                    })}
                    disabled={deleteRule.isPending}
                    onClick={() => void deleteRule.mutateAsync(rule.id)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
