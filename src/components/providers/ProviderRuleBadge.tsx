import { useTranslation } from "react-i18next";
import { Loader2, Sparkles, Undo2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useCancelSwitchRuleRevert } from "@/lib/query/switchRules";
import type { SwitchRuleState } from "@/lib/api/switchRules";

interface ProviderRuleBadgeProps {
  state: SwitchRuleState;
  className?: string;
}

/**
 * 当前供应商是额度规则自动切过来的：标出原因与回切时间，并提供「保持当前」
 * 取消待回切（与规则面板里的按钮等价）。
 */
export function ProviderRuleBadge({
  state,
  className,
}: ProviderRuleBadgeProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || "en";
  const cancelRevert = useCancelSwitchRuleRevert();

  const revertText = state.resetsAt
    ? t("provider.ruleBadge.revertAt", {
        defaultValue: "将于 {{time}} 切回",
        time: new Date(state.resetsAt * 1000).toLocaleString(locale),
      })
    : t("provider.ruleBadge.revertPending", { defaultValue: "等待切回" });
  const tooltip = `${t("provider.ruleBadge.tooltip", {
    defaultValue: "由额度规则自动切换（{{reason}}）",
    reason: state.reason ?? "-",
  })} · ${revertText}`;

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400",
        className,
      )}
      title={tooltip}
      data-testid="provider-rule-badge"
    >
      <Sparkles className="h-3 w-3" />
      {t("provider.ruleBadge.label", { defaultValue: "规则切换" })}
      <button
        type="button"
        className="ml-0.5 inline-flex items-center gap-0.5 rounded px-1 hover:bg-amber-500/20"
        disabled={cancelRevert.isPending}
        onClick={(event) => {
          event.stopPropagation();
          void cancelRevert.mutateAsync(state.ruleId);
        }}
        title={t("provider.ruleBadge.keepCurrentHint", {
          defaultValue: "取消自动切回，保持当前供应商",
        })}
        aria-label={t("provider.ruleBadge.keepCurrent", {
          defaultValue: "保持当前",
        })}
      >
        {cancelRevert.isPending ? (
          <Loader2 className="h-3 w-3 animate-spin" />
        ) : (
          <Undo2 className="h-3 w-3" />
        )}
        {t("provider.ruleBadge.keepCurrent", { defaultValue: "保持当前" })}
      </button>
    </span>
  );
}
