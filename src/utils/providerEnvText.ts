/**
 * Line-preserving `.env` editing for the Gemini env editor (FE-PROV-48).
 *
 * The editor is free-form text, but structured fields (API key, base URL,
 * model) and the common-config toggle used to rebuild it from an object,
 * dropping comments, blank lines and key order on every keystroke. These
 * helpers patch the text instead.
 */

/** Keys that are omitted from the text while empty (matches the editor). */
const OMIT_WHEN_EMPTY = new Set([
  "GOOGLE_GEMINI_BASE_URL",
  "GEMINI_API_KEY",
  "GEMINI_MODEL",
]);

const parseAssignment = (line: string): string | null => {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith("#")) return null;
  const eq = trimmed.indexOf("=");
  return eq > 0 ? trimmed.slice(0, eq).trim() : null;
};

/**
 * Rewrite `text` so it holds exactly the assignments in `env`: existing
 * `KEY=` lines are updated in place, keys missing from `env` are removed,
 * new keys are appended at the end, and comments, blank lines and unknown
 * lines are left untouched.
 */
export function mergeEnvIntoText(
  text: string,
  env: Record<string, unknown>,
): string {
  const remaining = new Map<string, string>();
  for (const [key, value] of Object.entries(env)) {
    if (typeof value !== "string") continue;
    if (value === "" && OMIT_WHEN_EMPTY.has(key)) continue;
    remaining.set(key, value);
  }

  const out: string[] = [];
  const written = new Set<string>();
  for (const line of text === "" ? [] : text.split("\n")) {
    const key = parseAssignment(line);
    if (key === null) {
      out.push(line);
      continue;
    }
    if (written.has(key) || !remaining.has(key)) continue;
    out.push(`${key}=${remaining.get(key)}`);
    remaining.delete(key);
    written.add(key);
  }
  for (const [key, value] of remaining) {
    // Keep a trailing blank line (if any) after the appended keys.
    if (out.length > 0 && out[out.length - 1] === "" && text.endsWith("\n")) {
      out.splice(out.length - 1, 0, `${key}=${value}`);
    } else {
      out.push(`${key}=${value}`);
    }
  }
  return out.join("\n");
}
