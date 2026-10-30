export const DEFAULT_MAX_LINES: number;
export function stripLiterals(source: string): string;
export function scanCommandBodies(
  source: string,
): { name: string; line: number; lines: number; dbAccess: boolean }[];
export function checkCommandBodies(
  files: Record<string, string>,
  maxLines?: number,
): {
  total: number;
  violations: { file: string; line: number; name: string; reason: string }[];
};
export function readCommandSources(root: string): Record<string, string>;
