import { readLegacyString, removeStoredKey } from "@/lib/storage";
import { configApi } from "@/lib/api";

export type CommonConfigApp = "claude" | "codex" | "gemini";

/** Pre-config.json localStorage keys of each app's common-config snippet. */
export const LEGACY_COMMON_CONFIG_KEYS: Record<CommonConfigApp, string> = {
  claude: "cc-switch:common-config-snippet",
  codex: "cc-switch:codex-common-config-snippet",
  gemini: "cc-switch:gemini-common-config-snippet",
};

/**
 * Load an app's common-config snippet: the stored one, or else a legacy
 * localStorage snippet, which is migrated into config.json and removed.
 * `acceptLegacy` can veto a legacy snippet that no longer fits the format
 * (it is then left in place). Returns `null` when there is no snippet.
 *
 * Shared by the Claude, Codex and Gemini common-config hooks, which used to
 * carry three copies of this lifecycle (FE-PROV-18).
 */
export async function loadCommonConfigSnippet(
  app: CommonConfigApp,
  acceptLegacy: (snippet: string) => boolean = () => true,
): Promise<string | null> {
  const snippet = await configApi.getCommonConfigSnippet(app);
  if (snippet && snippet.trim()) return snippet;
  if (typeof window === "undefined") return null;

  const key = LEGACY_COMMON_CONFIG_KEYS[app];
  try {
    const legacy = readLegacyString(key);
    if (!legacy || !legacy.trim()) return null;
    if (!acceptLegacy(legacy)) {
      console.warn(
        `[migration] legacy ${app} common config snippet no longer matches the format; skipped`,
      );
      return null;
    }
    await configApi.setCommonConfigSnippet(app, legacy);
    removeStoredKey(key);
    return legacy;
  } catch (e) {
    console.warn("[migration] localStorage migration failed:", e);
    return null;
  }
}
