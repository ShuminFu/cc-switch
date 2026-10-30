import { describe, expect, it } from "vitest";
import {
  createAppQueryClient,
  DEFAULT_STALE_TIME_MS,
} from "@/lib/query/queryClient";

describe("app query client defaults", () => {
  it("does not treat freshly fetched data as stale on window focus", () => {
    const client = createAppQueryClient();
    const defaults = client.getDefaultOptions().queries;
    expect(defaults?.staleTime).toBe(DEFAULT_STALE_TIME_MS);
    expect(DEFAULT_STALE_TIME_MS).toBeGreaterThanOrEqual(30_000);
    expect(defaults?.refetchOnWindowFocus).toBe(true);
    expect(client.getDefaultOptions().mutations?.retry).toBe(false);
  });
});
