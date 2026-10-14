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

/**
 * Strictly validate an integer-only field at submit time.
 *
 * Unlike {@link parseNonNegativeIntInput}, nothing is coerced: decimals,
 * negatives and garbage are rejected so the caller can tell the user instead
 * of silently saving a different value.
 *
 * @returns `undefined` for an empty field, `null` when the value is not a
 *   non-negative safe integer, otherwise the parsed integer ("1e3" → 1000).
 */
export function parseStrictNonNegativeInt(
  raw: string,
): number | null | undefined {
  const value = parseStrictInteger(raw);
  return value == null ? value : value >= 0 ? value : null;
}

/** Plain decimal literal, optionally with an exponent: no hex/binary/octal. */
const DECIMAL_LITERAL = /^[+-]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;

/**
 * Strictly parse a (possibly negative) integer typed by the user.
 *
 * @returns `undefined` for an empty field, `null` when the text is not a
 *   safe integer (decimals, garbage, "0x10", out of range), otherwise the
 *   integer. Exponent form is accepted when it denotes an integer
 *   ("1e5" → 100000).
 */
export function parseStrictInteger(raw: string): number | null | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (!DECIMAL_LITERAL.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) ? value : null;
}
