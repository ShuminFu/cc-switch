import type { StreamCheckConfig } from "@/lib/api/connectivity-check";

export type StreamCheckField = keyof StreamCheckConfig;
export type StreamCheckDraft = Record<StreamCheckField, string>;

/** Accepted ranges, matching the input `min`/`max` attributes. */
export const STREAM_CHECK_LIMITS: Record<
  StreamCheckField,
  { min: number; max: number }
> = {
  timeoutSecs: { min: 2, max: 60 },
  maxRetries: { min: 0, max: 5 },
  degradedThresholdMs: { min: 1000, max: 30000 },
};

const FIELDS: StreamCheckField[] = [
  "timeoutSecs",
  "maxRetries",
  "degradedThresholdMs",
];

/**
 * Parse the draft. Blank or out-of-range values are reported per field
 * instead of being silently replaced by defaults (FE-UI-10).
 */
export function parseStreamCheckDraft(draft: StreamCheckDraft): {
  config: StreamCheckConfig | null;
  invalid: StreamCheckField[];
} {
  const invalid: StreamCheckField[] = [];
  const parsed = {} as StreamCheckConfig;
  for (const key of FIELDS) {
    const raw = draft[key].trim();
    const n = Number(raw);
    const { min, max } = STREAM_CHECK_LIMITS[key];
    if (raw === "" || !Number.isInteger(n) || n < min || n > max) {
      invalid.push(key);
    } else {
      parsed[key] = n;
    }
  }
  return { config: invalid.length ? null : parsed, invalid };
}

export const toStreamCheckDraft = (
  config: StreamCheckConfig,
): StreamCheckDraft => ({
  timeoutSecs: String(config.timeoutSecs),
  maxRetries: String(config.maxRetries),
  degradedThresholdMs: String(config.degradedThresholdMs),
});
