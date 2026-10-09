import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  Eraser,
  History,
  Loader2,
  Power,
  PowerOff,
  RotateCcw,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
  Sparkles,
  ToggleLeft,
  ToggleRight,
  Undo2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useClearProxyEvents, useProxyEvents } from "@/lib/query/proxy";
import type { ProxyEvent } from "@/types/proxy";

const APP_FILTERS = ["all", "claude", "codex", "gemini", "grokbuild"] as const;
type AppFilter = (typeof APP_FILTERS)[number];

const APP_LABELS: Record<AppFilter, string> = {
  all: "All",
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  grokbuild: "Grok Build",
};

const KIND_META: Record<
  string,
  { icon: typeof Activity; tone: string; defaultLabel: string }
> = {
  proxy_start: {
    icon: Power,
    tone: "text-emerald-500",
    defaultLabel: "代理已启动",
  },
  proxy_stop: {
    icon: PowerOff,
    tone: "text-muted-foreground",
    defaultLabel: "代理已停止",
  },
  takeover_on: {
    icon: ToggleRight,
    tone: "text-primary",
    defaultLabel: "开启接管",
  },
  takeover_off: {
    icon: ToggleLeft,
    tone: "text-muted-foreground",
    defaultLabel: "关闭接管",
  },
  failover_switch: {
    icon: Activity,
    tone: "text-orange-500",
    defaultLabel: "故障转移切换",
  },
  breaker_open: {
    icon: ShieldAlert,
    tone: "text-red-500",
    defaultLabel: "熔断器打开",
  },
  breaker_half_open: {
    icon: ShieldQuestion,
    tone: "text-amber-500",
    defaultLabel: "熔断器半开（探测中）",
  },
  breaker_closed: {
    icon: ShieldCheck,
    tone: "text-emerald-500",
    defaultLabel: "熔断器关闭（已恢复）",
  },
  breaker_reset: {
    icon: RotateCcw,
    tone: "text-sky-500",
    defaultLabel: "手动重置熔断器",
  },
  rule_switch: {
    icon: Sparkles,
    tone: "text-amber-500",
    defaultLabel: "额度规则切换",
  },
  rule_revert: {
    icon: Undo2,
    tone: "text-emerald-500",
    defaultLabel: "额度规则回切",
  },
};

export function formatEventTime(createdAt: number, locale: string): string {
  return new Date(createdAt * 1000).toLocaleString(locale, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

interface ProxyEventTimelineProps {
  limit?: number;
  className?: string;
}

/** 代理事件时间线：toast 消失、应用重启之后仍能回答“为什么我现在在 P2” */
export function ProxyEventTimeline({
  limit = 50,
  className,
}: ProxyEventTimelineProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || "en";
  const [filter, setFilter] = useState<AppFilter>("all");
  const appType = filter === "all" ? undefined : filter;
  const { data: events = [], isLoading } = useProxyEvents(appType, { limit });
  const clearEvents = useClearProxyEvents();

  const rows = useMemo(() => events.slice(0, limit), [events, limit]);

  const kindLabel = (event: ProxyEvent) => {
    const meta = KIND_META[event.kind];
    return t(`proxy.events.kinds.${event.kind}`, {
      defaultValue: meta?.defaultLabel ?? event.kind,
    });
  };

  return (
    <div
      className={cn(
        "rounded-lg border border-border bg-muted/40 p-4 space-y-3",
        className,
      )}
      data-testid="proxy-event-timeline"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <History className="h-4 w-4 text-muted-foreground" />
          <h4 className="text-sm font-semibold">
            {t("proxy.events.title", { defaultValue: "最近事件" })}
          </h4>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex items-center rounded-md border border-border/60 bg-background p-0.5">
            {APP_FILTERS.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setFilter(option)}
                className={cn(
                  "rounded px-2 py-0.5 text-xs transition-colors",
                  filter === option
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:text-foreground",
                )}
                aria-pressed={filter === option}
              >
                {option === "all"
                  ? t("proxy.events.allApps", { defaultValue: "全部" })
                  : APP_LABELS[option]}
              </button>
            ))}
          </div>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-7 gap-1 px-2 text-xs"
            disabled={clearEvents.isPending || rows.length === 0}
            onClick={() => void clearEvents.mutateAsync(appType)}
          >
            <Eraser className="h-3.5 w-3.5" />
            {t("proxy.events.clear", { defaultValue: "清空" })}
          </Button>
        </div>
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("common.loading", { defaultValue: "加载中..." })}
        </div>
      ) : rows.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          {t("proxy.events.empty", {
            defaultValue:
              "还没有事件。故障转移、熔断、接管与额度规则的动作都会记录在这里。",
          })}
        </p>
      ) : (
        <ul className="max-h-72 space-y-1.5 overflow-y-auto pr-1">
          {rows.map((event) => {
            const meta = KIND_META[event.kind];
            const Icon = meta?.icon ?? Activity;
            return (
              <li
                key={event.id}
                className="flex items-start gap-2 rounded-md bg-background/60 px-2.5 py-1.5 text-xs"
                data-testid={`proxy-event-${event.id}`}
              >
                <Icon
                  className={cn("mt-0.5 h-3.5 w-3.5 shrink-0", meta?.tone)}
                />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-x-2">
                    <span className="font-medium">{kindLabel(event)}</span>
                    {event.appType !== "*" && (
                      <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] uppercase text-muted-foreground">
                        {event.appType}
                      </span>
                    )}
                    {(event.providerName || event.providerId) && (
                      <span className="truncate text-muted-foreground">
                        {event.providerName ?? event.providerId}
                      </span>
                    )}
                  </div>
                  {event.detail && (
                    <p
                      className="truncate text-muted-foreground"
                      title={event.detail}
                    >
                      {event.detail}
                    </p>
                  )}
                </div>
                <time
                  className="shrink-0 tabular-nums text-muted-foreground"
                  dateTime={new Date(event.createdAt * 1000).toISOString()}
                >
                  {formatEventTime(event.createdAt, locale)}
                </time>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
