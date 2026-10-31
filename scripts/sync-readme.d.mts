// Types for scripts/sync-readme.mjs (used by tests/scripts/syncReadme.test.ts).
export interface SponsorCell {
  href: string;
  logo: string;
  alt: string;
}
export interface Sponsor extends SponsorCell {
  blurb: Record<string, string>;
  overrides?: Record<string, Partial<SponsorCell>>;
}
export interface SponsorData {
  sponsors: Sponsor[];
}
export const DATA_FILE: string;
export const START: string;
export const END: string;
export const README_BY_LOCALE: Record<"en" | "zh" | "ja" | "de", string>;
export function parseTable(table: string): Array<SponsorCell & { blurb: string }>;
export function renderTable(data: SponsorData, locale: string): string;
export function readBlock(markdown: string): string | null;
export function writeBlock(markdown: string, table: string): string;
export function extract(readmes: Record<string, string>): SponsorData;
export function outOfSync(root?: string): string[];
