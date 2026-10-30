import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useProxyRequestTrace } from "@/lib/query/proxy";
import type {
  RequestTrace,
  TraceAttempt,
  TraceAttemptStatus,
  TraceHeader,
} from "@/types/proxy";

/** 后端环形缓冲区保留的请求数，与 MAX_TRACES 一致 */
export const REQUEST_TRACE_LIMIT = 100;

const ATTEMPT_STATUS_CLASS: Record<TraceAttemptStatus, string> = {
  succeeded: "bg-green-100 text-green-800",
  failed: "bg-red-100 text-red-800",
  skipped: "bg-amber-100 text-amber-800",
  in_flight: "bg-gray-100 text-gray-700",
};

const fmtMs = (ms: number | null | undefined) => (ms == null ? "—" : `${ms}ms`);

function HeaderList({
  title,
  headers,
}: {
  title: string;
  headers: TraceHeader[];
}) {
  if (headers.length === 0) return null;
  return (
    <details className="mt-2 text-xs">
      <summary className="cursor-pointer text-muted-foreground">
        {title} ({headers.length})
      </summary>
      <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 font-mono">
        {headers.map((header, index) => (
          <div key={`${header.name}-${index}`} className="contents">
            <dt className="text-muted-foreground">{header.name}</dt>
            <dd className="break-all">{header.value}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}

function AttemptCard({
  attempt,
  index,
}: {
  attempt: TraceAttempt;
  index: number;
}) {
  const { t } = useTranslation();
  const plan = attempt.plan;
  return (
    <li
      className="rounded-lg border p-3"
      data-testid="trace-attempt"
      data-status={attempt.status}
    >
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="font-mono text-muted-foreground">#{index + 1}</span>
        <span className="font-medium">
          {attempt.providerName || attempt.providerId}
        </span>
        <span className="font-mono text-xs text-muted-foreground">
          {attempt.providerId}
        </span>
        <span
          className={`inline-flex rounded-full px-2 py-0.5 text-xs ${ATTEMPT_STATUS_CLASS[attempt.status]}`}
        >
          {t(`usage.trace.status.${attempt.status}`)}
        </span>
        {attempt.upstreamStatus != null && (
          <span className="font-mono text-xs">
            HTTP {attempt.upstreamStatus}
          </span>
        )}
        <span className="ml-auto font-mono text-xs text-muted-foreground">
          +{attempt.startedMs}ms · {fmtMs(attempt.durationMs)}
        </span>
      </div>

      {attempt.reason && (
        <p className="mt-2 break-words text-xs text-red-700">
          <span className="font-medium">{t("usage.trace.reason")}: </span>
          {attempt.reason}
        </p>
      )}

      {attempt.recovery.length > 0 && (
        <p className="mt-1 text-xs">
          <span className="text-muted-foreground">
            {t("usage.trace.recovery")}:{" "}
          </span>
          <span className="font-mono">{attempt.recovery.join(" → ")}</span>
        </p>
      )}

      {plan && (
        <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
          <dt className="text-muted-foreground">{t("usage.trace.route")}</dt>
          <dd className="font-mono">
            {plan.client} → {plan.upstream}
            {plan.apiFormat ? ` (${plan.apiFormat})` : ""} · {plan.decode}
          </dd>
          <dt className="text-muted-foreground">URL</dt>
          <dd className="break-all font-mono">{plan.url}</dd>
          {plan.auth && (
            <>
              <dt className="text-muted-foreground">{t("usage.trace.auth")}</dt>
              <dd className="font-mono">{plan.auth}</dd>
            </>
          )}
          {plan.quirks.length + plan.sideStores.length > 0 && (
            <>
              <dt className="text-muted-foreground">
                {t("usage.trace.flags")}
              </dt>
              <dd className="font-mono">
                {[...plan.quirks, ...plan.sideStores].join(", ")}
              </dd>
            </>
          )}
        </dl>
      )}

      <HeaderList
        title={t("usage.trace.upstreamHeaders")}
        headers={attempt.upstreamHeaders}
      />
    </li>
  );
}

/** 请求追踪内容：概要、客户端请求头与每次尝试 */
export function RequestTraceView({ trace }: { trace: RequestTrace }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="rounded-lg border p-4">
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-3">
          <div>
            <dt className="text-muted-foreground">{t("usage.status")}</dt>
            <dd className="font-mono">{trace.statusCode ?? "—"}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">{t("usage.trace.ttfb")}</dt>
            <dd className="font-mono">{fmtMs(trace.ttfbMs)}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">
              {t("usage.trace.firstToken")}
            </dt>
            <dd className="font-mono">{fmtMs(trace.firstTokenMs)}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">{t("usage.latency")}</dt>
            <dd className="font-mono">{fmtMs(trace.latencyMs)}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">
              {t("usage.trace.usageSource")}
            </dt>
            <dd>
              {trace.usageSource
                ? t(`usage.trace.usageSourceValue.${trace.usageSource}`)
                : "—"}
            </dd>
          </div>
          <div>
            <dt className="text-muted-foreground">
              {t("usage.trace.request")}
            </dt>
            <dd className="break-all font-mono text-xs">
              {trace.method} {trace.path}
              {trace.isStream ? " · stream" : ""}
            </dd>
          </div>
        </dl>
        {trace.error && (
          <p className="mt-3 break-words text-xs text-red-700">{trace.error}</p>
        )}
        <HeaderList
          title={t("usage.trace.clientHeaders")}
          headers={trace.clientHeaders}
        />
      </div>

      <div>
        <h3 className="mb-2 font-semibold">
          {t("usage.trace.attempts", { count: trace.attempts.length })}
        </h3>
        {trace.attempts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("usage.trace.noAttempts")}
          </p>
        ) : (
          <ol className="space-y-2">
            {trace.attempts.map((attempt, index) => (
              <AttemptCard
                key={`${attempt.providerId}-${index}`}
                attempt={attempt}
                index={index}
              />
            ))}
          </ol>
        )}
        {trace.droppedAttempts > 0 && (
          <p className="mt-2 text-xs text-muted-foreground">
            {t("usage.trace.droppedAttempts", {
              count: trace.droppedAttempts,
            })}
          </p>
        )}
      </div>
    </div>
  );
}

interface RequestTracePanelProps {
  requestId: string;
  onClose: () => void;
}

/** 请求日志行的追踪详情面板：说明请求为何落到最终的 provider */
export function RequestTracePanel({
  requestId,
  onClose,
}: RequestTracePanelProps) {
  const { t } = useTranslation();
  const { data: trace, isLoading } = useProxyRequestTrace(requestId);

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("usage.trace.title")}</DialogTitle>
        </DialogHeader>
        <p className="font-mono text-xs text-muted-foreground">{requestId}</p>
        {isLoading ? (
          <div className="h-[240px] animate-pulse rounded bg-gray-100" />
        ) : trace ? (
          <RequestTraceView trace={trace} />
        ) : (
          <p className="text-center text-sm text-muted-foreground">
            {t("usage.trace.notFound", { count: REQUEST_TRACE_LIMIT })}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
