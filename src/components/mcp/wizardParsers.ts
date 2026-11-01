export interface ParsedPairs {
  values: Record<string, string>;
  /** Non-empty lines that could not be parsed (FE-UI-32: no silent drops). */
  invalidLines: string[];
}

const nonEmptyLines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

/** `KEY=VALUE` per line. */
export function parseEnvText(text: string): ParsedPairs {
  const values: Record<string, string> = {};
  const invalidLines: string[] = [];
  for (const line of nonEmptyLines(text)) {
    const idx = line.indexOf("=");
    const key = idx > 0 ? line.slice(0, idx).trim() : "";
    if (!key) {
      invalidLines.push(line);
      continue;
    }
    values[key] = line.slice(idx + 1).trim();
  }
  return { values, invalidLines };
}

/** `KEY: VALUE` or `KEY=VALUE` per line (whichever separator comes first). */
export function parseHeadersText(text: string): ParsedPairs {
  const values: Record<string, string> = {};
  const invalidLines: string[] = [];
  for (const line of nonEmptyLines(text)) {
    const colonIdx = line.indexOf(":");
    const equalIdx = line.indexOf("=");
    let idx = -1;
    if (colonIdx > 0 && (equalIdx === -1 || colonIdx < equalIdx)) {
      idx = colonIdx;
    } else if (equalIdx > 0) {
      idx = equalIdx;
    }
    const key = idx > 0 ? line.slice(0, idx).trim() : "";
    if (!key) {
      invalidLines.push(line);
      continue;
    }
    values[key] = line.slice(idx + 1).trim();
  }
  return { values, invalidLines };
}
