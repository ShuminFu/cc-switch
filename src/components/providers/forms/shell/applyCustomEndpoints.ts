import { endpointsApi, type AppId } from "@/lib/api";

const normalize = (url: string) => url.trim().replace(/\/+$/, "");

/**
 * Make the stored custom speed-test endpoints of a provider equal `urls`.
 *
 * Called by the edit dialog only after the provider itself was saved, so the
 * endpoint panel's changes follow the dialog's Save/Cancel (FE-PROV-10).
 * Removals run before additions; the first failure stops and rethrows.
 */
export async function applyCustomEndpoints(
  appId: AppId,
  providerId: string,
  urls: string[],
): Promise<{ added: string[]; removed: string[] }> {
  const stored = await endpointsApi.getCustomEndpoints(appId, providerId);
  const current = new Set(stored.map((ep) => normalize(ep.url)));
  const next = new Set(urls.map(normalize).filter(Boolean));
  const removed = [...current].filter((url) => !next.has(url));
  const added = [...next].filter((url) => !current.has(url));
  for (const url of removed) {
    await endpointsApi.removeCustomEndpoint(appId, providerId, url);
  }
  for (const url of added) {
    await endpointsApi.addCustomEndpoint(appId, providerId, url);
  }
  return { added, removed };
}
