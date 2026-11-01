import { z } from "zod";

/** Valid ranges of the per-app failover / circuit-breaker fields (FE-UI-20). */
export const AUTO_FAILOVER_RANGES = {
  maxRetries: { min: 0, max: 10 },
  streamingFirstByteTimeout: { min: 1, max: 120 },
  streamingIdleTimeout: { min: 0, max: 600 },
  nonStreamingTimeout: { min: 60, max: 1200 },
  circuitFailureThreshold: { min: 1, max: 20 },
  circuitSuccessThreshold: { min: 1, max: 10 },
  circuitTimeoutSeconds: { min: 0, max: 300 },
  /** Percentage in the form; stored as a 0-1 ratio. */
  circuitErrorRateThreshold: { min: 0, max: 100 },
  circuitMinRequests: { min: 5, max: 100 },
} as const;

export type AutoFailoverNumericField = keyof typeof AUTO_FAILOVER_RANGES;

const integerInRange = ({ min, max }: { min: number; max: number }) =>
  z
    .string()
    .trim()
    .regex(/^-?\d+$/)
    .transform((value) => parseInt(value, 10))
    .pipe(z.number().int().min(min).max(max));

export const autoFailoverFormSchema = z.object(
  Object.fromEntries(
    Object.entries(AUTO_FAILOVER_RANGES).map(([field, range]) => [
      field,
      integerInRange(range),
    ]),
  ) as Record<AutoFailoverNumericField, ReturnType<typeof integerInRange>>,
);

export type AutoFailoverNumbers = Record<AutoFailoverNumericField, number>;

export type AutoFailoverValidation =
  | { success: true; data: AutoFailoverNumbers }
  | { success: false; invalidFields: AutoFailoverNumericField[] };

/** Validate the string inputs of the form; returns the offending fields. */
export function validateAutoFailoverForm(
  form: Record<AutoFailoverNumericField, string>,
): AutoFailoverValidation {
  const result = autoFailoverFormSchema.safeParse(form);
  if (result.success) {
    return { success: true, data: result.data as AutoFailoverNumbers };
  }
  const invalidFields = Array.from(
    new Set(
      result.error.issues.map(
        (issue) => issue.path[0] as AutoFailoverNumericField,
      ),
    ),
  );
  return { success: false, invalidFields };
}
