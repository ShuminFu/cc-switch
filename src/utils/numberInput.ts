/**
 * Parse the raw value of an integer-only <input> (token counts, context
 * windows, …) into a non-negative safe integer.
 *
 * `parseInt` is unsuitable here: it turns "1e5" into 1 and silently accepts
 * trailing garbage such as "12abc". Instead:
 * - empty / whitespace → `undefined` (field cleared)
 * - any non-negative numeric literal (incl. exponent form) → floored
 *   integer, so "1e5" → 100000 and "1.9" → 1; out-of-range → `undefined`
 * - anything else → non-digit characters are stripped, matching how the
 *   Codex max-output-tokens field sanitizes input ("12abc" → 12)
 */
export function parseNonNegativeIntInput(raw: string): number | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;

  const asNumber = Number(trimmed);
  if (!Number.isNaN(asNumber) && asNumber >= 0) {
    const floored = Math.floor(asNumber);
    return Number.isSafeInteger(floored) ? floored : undefined;
  }

  const digits = trimmed.replace(/\D/g, "");
  if (!digits) return undefined;
  const parsed = Number(digits);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}
