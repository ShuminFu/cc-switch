import type { TakeoverReapplyResult } from "@/types/proxy";

export type ReapplySummary =
  | { kind: "nothing" }
  | { kind: "ok"; apps: string[] }
  | { kind: "partial"; okApps: string[]; failures: TakeoverReapplyResult[] };

/** 把逐应用的重建结果折叠成一条提示 */
export function summarizeReapply(
  results: TakeoverReapplyResult[],
): ReapplySummary {
  if (results.length === 0) return { kind: "nothing" };
  const failures = results.filter((r) => !r.ok);
  const okApps = results.filter((r) => r.ok).map((r) => r.appType);
  if (failures.length === 0) return { kind: "ok", apps: okApps };
  return { kind: "partial", okApps, failures };
}
