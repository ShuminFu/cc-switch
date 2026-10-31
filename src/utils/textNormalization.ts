import { parse as parseToml } from "smol-toml";

const SMART_DOUBLE = new Set(["“", "”", "„", "‟", "＂"]);
const SMART_SINGLE = new Set(["‘", "’", "＇"]);

/**
 * Replace every curly / full-width quote with its ASCII counterpart,
 * regardless of context. Only safe for text that is not TOML (or when the
 * whole text is known to use smart quotes as delimiters).
 * - double: “ ” „ ‟ ＂ → "
 * - single: ‘ ’ ＇ → '
 */
export const normalizeQuotes = (text: string): string => {
  if (!text) return text;
  return text.replace(/[“”„‟＂]/g, '"').replace(/[‘’＇]/g, "'");
};

/**
 * Replace smart quotes only where TOML would read a string delimiter
 * (FE-ARCH-37). Quotes inside ASCII-delimited strings and comments are user
 * content and stay byte-identical: `x = 'Don’t'` and `x = "“hi”"` are not
 * touched, while `x = “hi”` becomes `x = "hi"`.
 *
 * A string opened by a smart quote is closed by the next quote of the same
 * family (smart or ASCII), so `“hi”` is recognised as one string.
 */
export const normalizeTomlText = (text: string): string => {
  if (!text) return text;
  let out = "";
  let i = 0;
  const n = text.length;
  while (i < n) {
    const ch = text[i];
    if (ch === "#") {
      const end = text.indexOf("\n", i);
      const stop = end === -1 ? n : end;
      out += text.slice(i, stop);
      i = stop;
      continue;
    }
    const isDouble = ch === '"' || SMART_DOUBLE.has(ch);
    const isSingle = ch === "'" || SMART_SINGLE.has(ch);
    if (!isDouble && !isSingle) {
      out += ch;
      i += 1;
      continue;
    }
    const ascii = isDouble ? '"' : "'";
    const smartFamily = isDouble ? SMART_DOUBLE : SMART_SINGLE;
    const openedBySmart = ch !== ascii;
    // Multi-line strings ("""...""" / '''...''') opened with ASCII quotes.
    if (!openedBySmart && text.startsWith(ascii.repeat(3), i)) {
      const close = text.indexOf(ascii.repeat(3), i + 3);
      const stop = close === -1 ? n : close + 3;
      out += text.slice(i, stop);
      i = stop;
      continue;
    }
    out += ascii;
    i += 1;
    while (i < n) {
      const c = text[i];
      if (c === "\n") break; // single-line strings end at the line
      if (isDouble && c === "\\" && !openedBySmart) {
        out += text.slice(i, i + 2);
        i += 2;
        continue;
      }
      if (c === ascii || (openedBySmart && smartFamily.has(c))) {
        out += ascii;
        i += 1;
        break;
      }
      out += c;
      i += 1;
    }
  }
  return out;
};

/** True when `text` uses smart quotes where TOML expects delimiters. */
export const hasSmartQuoteDelimiters = (text: string): boolean =>
  !!text && normalizeTomlText(text) !== text;

/**
 * Parse TOML as written; only when that fails, retry with smart-quote
 * delimiters replaced. The input text itself is never rewritten.
 */
export const parseTomlLenient = (text: string): Record<string, unknown> => {
  try {
    return parseToml(text) as Record<string, unknown>;
  } catch (error) {
    const fixed = normalizeTomlText(text);
    if (fixed === text) throw error;
    return parseToml(fixed) as Record<string, unknown>;
  }
};
