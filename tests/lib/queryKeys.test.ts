import { describe, it, expect } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import { liveProviderIdsKey, queryKeys, usageKeys } from "@/lib/query/keys";
import { usageKeys as reexportedUsageKeys } from "@/lib/query/usage";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";

describe("query key registry", () => {
  it("keeps the usage-script key out of the usage dashboard namespace", () => {
    const script = usageKeys.script("p1", "claude");
    expect(script[0]).not.toBe(usageKeys.all[0]);
    expect(queryKeys.usageScript.byProvider("p1", "claude")).toEqual(script);
    expect(reexportedUsageKeys).toBe(usageKeys);
  });

  it("invalidating the usage dashboard does not invalidate usage-script queries", async () => {
    const client = new QueryClient();
    const scriptKey = usageKeys.script("p1", "claude");
    const summaryKey = usageKeys.summary("today", undefined, undefined);
    client.setQueryData(scriptKey, { success: true });
    client.setQueryData(summaryKey, { total: 1 });

    await client.invalidateQueries({ queryKey: usageKeys.all });

    expect(client.getQueryState(summaryKey)?.isInvalidated).toBe(true);
    expect(client.getQueryState(scriptKey)?.isInvalidated).toBe(false);
  });

  it("nests per-app provider keys under the providers root", async () => {
    const client = new QueryClient();
    client.setQueryData(queryKeys.providers.byApp("codex"), {});
    await client.invalidateQueries({ queryKey: queryKeys.providers.all });
    expect(
      client.getQueryState(queryKeys.providers.byApp("codex"))?.isInvalidated,
    ).toBe(true);
  });

  it("is the source of the openclaw / hermes key factories", () => {
    expect(openclawKeys).toBe(queryKeys.openclaw);
    expect(hermesKeys).toBe(queryKeys.hermes);
  });

  it("maps additive-config apps to their live provider id keys", () => {
    expect(liveProviderIdsKey("opencode")).toEqual(["opencodeLiveProviderIds"]);
    expect(liveProviderIdsKey("openclaw")).toBe(
      queryKeys.openclaw.liveProviderIds,
    );
    expect(liveProviderIdsKey("hermes")).toBe(queryKeys.hermes.liveProviderIds);
    expect(liveProviderIdsKey("codex")).toBeNull();
  });
});
