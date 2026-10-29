import fs from "node:fs";
import path from "node:path";
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { providerKeys } from "@/entities/provider/keys";
import { invalidateProviderGraph } from "@/entities/provider/invalidate";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";
import { omoKeys, omoSlimKeys } from "@/lib/query/omo";
import { usageKeys } from "@/lib/query/usage";
import type { AppId } from "@/lib/api";

const SRC_DIR = path.resolve(__dirname, "..", "..", "src");
const KEYS_FILE = path.join(SRC_DIR, "entities", "provider", "keys.ts");

function listSourceFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx)$/.test(entry.name) ? [full] : [];
  });
}

function invalidatedKeys(appId: AppId, providerIds?: string[]) {
  const queryClient = new QueryClient();
  const spy = vi.spyOn(queryClient, "invalidateQueries");
  return invalidateProviderGraph(queryClient, appId, { providerIds }).then(() =>
    spy.mock.calls.map(([filters]) => filters?.queryKey),
  );
}

describe("providerKeys", () => {
  it("builds list keys under the shared providers prefix", () => {
    expect(providerKeys.all).toEqual(["providers"]);
    expect(providerKeys.list("codex")).toEqual(["providers", "codex"]);
    expect(providerKeys.list("codex").slice(0, 1)).toEqual(providerKeys.all);
  });

  it('is the only place in src that spells a ["providers" key', () => {
    const offenders = listSourceFiles(SRC_DIR)
      .filter((file) => file !== KEYS_FILE)
      .filter((file) => fs.readFileSync(file, "utf8").includes('["providers"'))
      .map((file) => path.relative(SRC_DIR, file));
    expect(offenders).toEqual([]);
  });
});

describe("invalidateProviderGraph", () => {
  it("invalidates only the provider list for apps without dependent caches", async () => {
    expect(await invalidatedKeys("claude")).toEqual([
      providerKeys.list("claude"),
    ]);
  });

  it("invalidates usage-script queries for each distinct provider id", async () => {
    const keys = await invalidatedKeys("codex", ["a", "b", "a"]);
    expect(keys).toEqual([
      providerKeys.list("codex"),
      usageKeys.script("a", "codex"),
      usageKeys.script("b", "codex"),
    ]);
  });

  it("covers OpenCode live ids and both OMO variants", async () => {
    const keys = await invalidatedKeys("opencode");
    expect(keys).toEqual(
      expect.arrayContaining([
        providerKeys.list("opencode"),
        providerKeys.opencodeLiveIds,
        omoKeys.all,
        omoSlimKeys.all,
      ]),
    );
  });

  it("covers OpenClaw live ids, default model and health", async () => {
    const keys = await invalidatedKeys("openclaw");
    expect(keys).toEqual(
      expect.arrayContaining([
        providerKeys.list("openclaw"),
        openclawKeys.liveProviderIds,
        openclawKeys.defaultModel,
        openclawKeys.health,
      ]),
    );
  });

  it("covers Hermes provider caches", async () => {
    const keys = await invalidatedKeys("hermes");
    expect(keys).toEqual(
      expect.arrayContaining([
        providerKeys.list("hermes"),
        hermesKeys.liveProviderIds,
        hermesKeys.modelConfig,
      ]),
    );
  });

  it("covers Claude Desktop status and proxy status", async () => {
    const keys = await invalidatedKeys("claude-desktop");
    expect(keys).toEqual(
      expect.arrayContaining([
        providerKeys.list("claude-desktop"),
        providerKeys.claudeDesktopStatus,
        ["proxyStatus"],
      ]),
    );
  });

  it("marks cached dependent queries stale", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(omoKeys.currentProviderId(), "omo-1");
    queryClient.setQueryData(providerKeys.list("claude"), { providers: {} });

    await invalidateProviderGraph(queryClient, "opencode");

    expect(
      queryClient.getQueryState(omoKeys.currentProviderId())?.isInvalidated,
    ).toBe(true);
    expect(
      queryClient.getQueryState(providerKeys.list("claude"))?.isInvalidated,
    ).toBe(false);
  });
});
