import type { LogFilters } from "@/types/usage";

/**
 * Request-log status filter (FE-UI-59): status classes plus the codes proxy
 * failures commonly return. Classes map to an inclusive `statusMin`/`statusMax`
 * range, single codes to an exact `statusCode`.
 */
export const STATUS_CLASS_OPTIONS = ["errors", "2xx", "4xx", "5xx"] as const;
export const STATUS_CODE_OPTIONS = [
  200, 400, 401, 403, 404, 408, 429, 500, 502, 503, 504, 529,
] as const;

export type StatusFilterValue =
  | "all"
  | (typeof STATUS_CLASS_OPTIONS)[number]
  | string;

export type StatusFilterFields = Pick<
  LogFilters,
  "statusCode" | "statusMin" | "statusMax"
>;

export function statusFilterToFilters(
  value: StatusFilterValue,
): StatusFilterFields {
  switch (value) {
    case "all":
      return {};
    case "errors":
      return { statusMin: 400 };
    case "2xx":
      return { statusMin: 200, statusMax: 299 };
    case "4xx":
      return { statusMin: 400, statusMax: 499 };
    case "5xx":
      return { statusMin: 500, statusMax: 599 };
    default: {
      const code = Number(value);
      return Number.isInteger(code) && code >= 100 && code <= 599
        ? { statusCode: code }
        : {};
    }
  }
}
