// Types for scripts/roadmap-status.mjs (used by tests/scripts/roadmapStatus.test.ts).
export interface Finding {
  id: string;
  title: string;
  area: string;
  priority: string;
}
export interface Commit {
  hash: string;
  subject: string;
  body: string;
}
export interface Classified {
  status: string;
  commits: string[];
  note: string;
}
export interface Override {
  status: string;
  note?: string;
  commits?: string[];
}
export const BASE: string;
export function parseFindings(files: Array<[string, string]>): Finding[];
export function classify(
  known: Set<string>,
  commits: Commit[],
): Map<string, Classified>;
export function render(
  findings: Finding[],
  classified: Map<string, Classified>,
  overrides: Record<string, Override>,
  base?: string,
): string;
