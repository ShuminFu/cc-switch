import type { UnpricedModelStats } from "@/types/usage";

const CSV_HEADER = [
  "model",
  "request_count",
  "input_tokens",
  "output_tokens",
  "cache_read_tokens",
  "cache_creation_tokens",
  "first_seen_at",
  "last_seen_at",
];

/** RFC 4180 转义；以 =+-@ 开头的值加单引号前缀，防止表格软件当公式执行 */
function csvCell(value: string | number): string {
  let text = String(value);
  if (typeof value === "string" && /^[=+\-@\t\r]/.test(text)) {
    text = `'${text}`;
  }
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function isoTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString();
}

/** 未计价请求按模型分组导出的 CSV 内容（含表头，CRLF 换行） */
export function buildUnpricedModelsCsv(stats: UnpricedModelStats[]): string {
  const rows = stats.map((s) =>
    [
      s.model,
      s.requestCount,
      s.inputTokens,
      s.outputTokens,
      s.cacheReadTokens,
      s.cacheCreationTokens,
      isoTime(s.firstSeenAt),
      isoTime(s.lastSeenAt),
    ]
      .map(csvCell)
      .join(","),
  );
  return [CSV_HEADER.join(","), ...rows].join("\r\n") + "\r\n";
}

/** 默认文件名，如 `unpriced-models-2026-10-30.csv` */
export function unpricedCsvFileName(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `unpriced-models-${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}.csv`;
}
