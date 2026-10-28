export const PROXY_PREFIX: string;
export const FIXTURE_PREFIX: string;
export const SKIP_LABEL: string;

export function parseLabels(raw: string | undefined): string[];
export function checkProxyFixture(input: {
  files: string[];
  labels: string[];
}): {
  ok: boolean;
  proxyFiles: string[];
  fixtureFiles: string[];
  skipped: boolean;
};
export function changedFiles(
  base: string,
  head: string,
  cwd?: string,
): string[];
