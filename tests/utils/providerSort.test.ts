import { describe, expect, it } from "vitest";
import {
  compareProviders,
  providerSortLocale,
  sortProviders,
  type SortableProvider,
} from "@/utils/providerSort";

const p = (
  id: string,
  name: string,
  sortIndex?: number,
  createdAt?: number,
): SortableProvider => ({ id, name, sortIndex, createdAt });

// FE-ARCH-35: one ordering for the query and drag-sort paths.
describe("sortProviders", () => {
  const mixed = [
    p("e", "Echo"),
    p("d", "Delta", undefined, 200),
    p("c", "Charlie", 1),
    p("b", "Bravo", undefined, 100),
    p("a", "Alpha", 0),
    p("f", "Alpha"),
  ];

  it("orders by sortIndex, then createdAt, then name, then id", () => {
    expect(sortProviders(mixed, "en-US").map((x) => x.id)).toEqual([
      "a",
      "c",
      "f",
      "e",
      "b",
      "d",
    ]);
  });

  it("gives the same order for arrays and id maps", () => {
    const map = Object.fromEntries(mixed.map((x) => [x.id, x]));
    const reversed = [...mixed].reverse();
    expect(sortProviders(map, "en-US")).toEqual(
      sortProviders(reversed, "en-US"),
    );
  });

  it("treats one-sided createdAt as 0 rather than skipping it", () => {
    expect(
      compareProviders(p("x", "Z", undefined, 5), p("y", "A"), "en-US"),
    ).toBeGreaterThan(0);
  });

  it("maps i18n languages to collation locales", () => {
    expect(providerSortLocale("zh")).toBe("zh-CN");
    expect(providerSortLocale("zh-TW")).toBe("zh-TW");
    expect(providerSortLocale("en")).toBe("en-US");
  });
});
