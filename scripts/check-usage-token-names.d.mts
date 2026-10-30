export const TOKEN_NAMES: RegExp;
export const ALLOWED_DIRS: string[];
export const ALLOWLIST: Record<string, string>;
export function firstTokenLine(source: string): number;
export function checkUsageTokenNames(
  files: Record<string, string>,
  allowlist?: Record<string, string>,
): {
  violations: { file: string; line: number; reason: string }[];
  stale: string[];
};
export function readRustSources(root: string): Record<string, string>;
