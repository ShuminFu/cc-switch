import { usageApi } from "@/lib/api/usage";
import type { LogFilters, RequestLog } from "@/types/usage";

/**
 * Usage export (FE-UI-25 / PRODUCT-50): the request logs of the active
 * dashboard filter as CSV or JSON. Rows are fetched with the same
 * `get_request_logs` command the table pages through, so the row count
 * equals the table's `total`.
 */

export type UsageExportFormat = "csv" | "json";

/** Page size used while collecting every row of the filter. */
export const EXPORT_PAGE_SIZE = 500;
/** Safety cap so a runaway filter cannot freeze the webview. */
export const EXPORT_MAX_ROWS = 200_000;

export const USAGE_CSV_COLUMNS: ReadonlyArray<{
  header: string;
  value: (log: RequestLog) => string | number | undefined | null;
}> = [
  { header: "time", value: (l) => new Date(l.createdAt * 1000).toISOString() },
  { header: "request_id", value: (l) => l.requestId },
  { header: "app", value: (l) => l.appType },
  { header: "provider", value: (l) => l.providerName ?? l.providerId },
  { header: "request_model", value: (l) => l.requestModel },
  { header: "billing_model", value: (l) => l.pricingModel ?? l.model },
  { header: "model", value: (l) => l.model },
  { header: "input_tokens", value: (l) => l.inputTokens },
  { header: "output_tokens", value: (l) => l.outputTokens },
  { header: "cache_read_tokens", value: (l) => l.cacheReadTokens },
  { header: "cache_creation_tokens", value: (l) => l.cacheCreationTokens },
  { header: "input_cost_usd", value: (l) => l.inputCostUsd },
  { header: "output_cost_usd", value: (l) => l.outputCostUsd },
  { header: "cache_read_cost_usd", value: (l) => l.cacheReadCostUsd },
  { header: "cache_creation_cost_usd", value: (l) => l.cacheCreationCostUsd },
  { header: "total_cost_usd", value: (l) => l.totalCostUsd },
  { header: "cost_multiplier", value: (l) => l.costMultiplier },
  { header: "latency_ms", value: (l) => l.latencyMs },
  { header: "first_token_ms", value: (l) => l.firstTokenMs },
  { header: "duration_ms", value: (l) => l.durationMs },
  { header: "streaming", value: (l) => (l.isStreaming ? "true" : "false") },
  { header: "status", value: (l) => l.statusCode },
  { header: "source", value: (l) => l.dataSource ?? "proxy" },
  { header: "error", value: (l) => l.errorMessage },
];

/** RFC 4180 field quoting; also neutralises spreadsheet formula injection. */
export function csvField(value: string | number | undefined | null): string {
  if (value === undefined || value === null) return "";
  let text = String(value);
  if (typeof value === "string" && /^[=+\-@\t\r]/.test(text)) {
    // A leading quote makes spreadsheets treat the cell as text.
    text = `'${text}`;
  }
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export function logsToCsv(logs: RequestLog[]): string {
  const lines = [USAGE_CSV_COLUMNS.map((c) => c.header).join(",")];
  for (const log of logs) {
    lines.push(USAGE_CSV_COLUMNS.map((c) => csvField(c.value(log))).join(","));
  }
  return `${lines.join("\r\n")}\r\n`;
}

export function logsToJson(logs: RequestLog[]): string {
  return `${JSON.stringify(logs, null, 2)}\n`;
}

/** Collect every row matching `filters` (already including the date range). */
export async function fetchAllRequestLogs(
  filters: LogFilters,
  pageSize: number = EXPORT_PAGE_SIZE,
): Promise<{ logs: RequestLog[]; total: number }> {
  const logs: RequestLog[] = [];
  let total = 0;
  for (let page = 0; ; page += 1) {
    const result = await usageApi.getRequestLogs(filters, page, pageSize);
    total = result.total;
    logs.push(...result.data);
    if (
      result.data.length < pageSize ||
      logs.length >= total ||
      logs.length >= EXPORT_MAX_ROWS
    ) {
      break;
    }
  }
  return { logs, total };
}

export function defaultExportFileName(
  format: UsageExportFormat,
  now: Date = new Date(),
): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  return `cc-switch-usage-${stamp}.${format}`;
}
