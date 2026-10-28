export interface MetainfoRelease {
  version: string;
  date: string;
}

export const METAINFO: string;
export const MAX_RELEASES: number;
export function changelogReleases(changelog: string): MetainfoRelease[];
export function renderReleases(releases: MetainfoRelease[]): string;
export function applyReleases(xml: string, block: string): string;
export function syncMetainfo(input: {
  xml: string;
  version: string;
  changelog: string;
}): string;
