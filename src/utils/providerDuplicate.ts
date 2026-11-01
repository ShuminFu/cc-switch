import type { Provider, ProviderMeta } from "@/types";
import { deepClone } from "@/utils/deepClone";

/**
 * Meta fields a duplicated provider must not inherit (FE-ARCH-43):
 * - `promptCacheKey` routes prompt-cache hits; two providers sharing it
 *   would share an upstream cache bucket.
 * - `isPartner` / `partnerPromotionKey` describe the sponsored preset the
 *   original was created from, not the user's copy.
 */
export const NON_COPYABLE_META_KEYS = [
  "promptCacheKey",
  "partnerPromotionKey",
  "isPartner",
] as const satisfies readonly (keyof ProviderMeta)[];

export type DuplicateProviderInput = Omit<Provider, "id" | "createdAt"> & {
  providerKey?: string;
  addToLive?: boolean;
};

/** A copy of `provider` ready for `addProvider`, placed right after it. */
export function buildDuplicateProvider(
  provider: Provider,
  name: string,
): DuplicateProviderInput {
  let meta: ProviderMeta | undefined;
  if (provider.meta) {
    meta = deepClone(provider.meta);
    for (const key of NON_COPYABLE_META_KEYS) {
      delete meta[key];
    }
  }
  return {
    name,
    settingsConfig: deepClone(provider.settingsConfig),
    websiteUrl: provider.websiteUrl,
    category: provider.category,
    sortIndex:
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined,
    meta,
    icon: provider.icon,
    iconColor: provider.iconColor,
  };
}

/** `<id>-copy`, `<id>-copy-2`, ... not present in `existingKeys`. */
export function generateUniqueProviderCopyKey(
  originalKey: string,
  existingKeys: string[],
): string {
  const baseKey = `${originalKey}-copy`;
  if (!existingKeys.includes(baseKey)) return baseKey;
  let counter = 2;
  while (existingKeys.includes(`${baseKey}-${counter}`)) counter++;
  return `${baseKey}-${counter}`;
}

/**
 * Sort-index bumps that make room for a copy inserted at `insertedIndex`:
 * every provider at or after that slot except the original and the copy
 * itself moves down by one.
 */
export function sortShiftForInsert(
  providers: Record<string, Provider>,
  insertedIndex: number,
  excludeIds: string[],
): { id: string; sortIndex: number }[] {
  return Object.values(providers)
    .filter(
      (p) =>
        p.sortIndex !== undefined &&
        p.sortIndex >= insertedIndex &&
        !excludeIds.includes(p.id),
    )
    .map((p) => ({ id: p.id, sortIndex: (p.sortIndex as number) + 1 }));
}
