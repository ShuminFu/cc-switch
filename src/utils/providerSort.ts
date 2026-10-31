import type { Provider } from "@/types";

/**
 * The one provider ordering used by the providers query and the drag-sort
 * hook (FE-ARCH-35): `sortIndex` ascending (missing last), then `createdAt`
 * ascending (missing first, as 0), then name in the UI locale.
 */
export type SortableProvider = Pick<
  Provider,
  "id" | "name" | "sortIndex" | "createdAt"
>;

/** BCP-47 collation locale for an i18n language code. */
export function providerSortLocale(language: string | undefined): string {
  if (language === "zh") return "zh-CN";
  if (language === "zh-TW") return "zh-TW";
  if (language === "ja") return "ja-JP";
  return "en-US";
}

export function compareProviders(
  a: SortableProvider,
  b: SortableProvider,
  locale: string,
): number {
  const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
  const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
  if (indexA !== indexB) return indexA - indexB;

  const timeA = a.createdAt ?? 0;
  const timeB = b.createdAt ?? 0;
  if (timeA !== timeB) return timeA - timeB;

  const byName = (a.name ?? "").localeCompare(b.name ?? "", locale);
  if (byName !== 0) return byName;
  // Deterministic final tie-break so both call sites agree on equal names.
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** Sorted copy of `providers` (array or id map). */
export function sortProviders<P extends SortableProvider>(
  providers: Record<string, P> | P[],
  locale: string,
): P[] {
  const list = Array.isArray(providers)
    ? [...providers]
    : Object.values(providers);
  return list.sort((a, b) => compareProviders(a, b, locale));
}
