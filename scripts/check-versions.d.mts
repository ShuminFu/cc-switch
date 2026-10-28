export type ManifestVersions = Record<string, string | null>;

export function cargoPackageVersion(text: string): string | null;
export function readVersions(root: string): {
  versions: ManifestVersions;
  changelog: string;
};
export function hasChangelogEntry(changelog: string, version: string): boolean;
export function checkVersions(input: {
  tag: string | null;
  versions: ManifestVersions;
  changelog: string;
}): string[];
