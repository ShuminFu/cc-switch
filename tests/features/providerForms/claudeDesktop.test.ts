import { describe, expect, it } from "vitest";
import { claudeDesktopProviderFormSchema } from "@/features/providers/forms/claude-desktop/schema";
import {
  claudeDesktopFormFromProvider,
  envString,
} from "@/features/providers/forms/claude-desktop/fromProvider";
import {
  toClaudeDesktopOfficialSave,
  toClaudeDesktopSave,
  type ClaudeDesktopSaveInput,
} from "@/features/providers/forms/claude-desktop/toSettingsConfig";
import {
  createRouteRow,
  isClaudeSafeRoute,
} from "@/features/providers/forms/claude-desktop/routes";
import {
  claudeDesktopPresetEntries,
  claudeDesktopPresetLoader,
} from "@/features/providers/forms/claude-desktop/presets";
import { CLAUDE_DESKTOP_ROLE_ROUTE_IDS } from "@/config/claudeDesktopRoles";

const row = (
  route: string,
  model = "",
  labelOverride = "",
  supports1m = false,
) => createRouteRow({ route, model, labelOverride, supports1m });

const saveInput = (
  overrides: Partial<ClaudeDesktopSaveInput>,
): ClaudeDesktopSaveInput => ({
  mode: "direct",
  apiFormat: "anthropic",
  baseUrl: " https://relay.example.com/// ",
  apiKey: " sk-1 ",
  apiKeyField: "ANTHROPIC_AUTH_TOKEN",
  usesManagedOAuth: false,
  routes: [],
  githubAccountId: null,
  codexAccountId: null,
  codexFastMode: false,
  ...overrides,
});

describe("claude-desktop form module (CCS-191)", () => {
  it("schema is the claude-desktop branch of the union", () => {
    expect(
      claudeDesktopProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "claude-desktop",
      }).success,
    ).toBe(true);
  });

  describe("fromProvider", () => {
    it("starts a new provider in direct mode with an empty env", () => {
      const state = claudeDesktopFormFromProvider(undefined);
      expect(state.mode).toBe("direct");
      expect(JSON.parse(state.defaultValues.settingsConfig)).toEqual({
        env: {},
      });
      expect(state.routes).toEqual([]);
      expect(state.hasSavedRoutes).toBe(false);
      expect(state.apiKeyField).toBe("ANTHROPIC_AUTH_TOKEN");
    });

    it("reads env, key field and proxy routes (normalized to four roles)", () => {
      const state = claudeDesktopFormFromProvider({
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://relay.example.com",
            ANTHROPIC_API_KEY: "sk-2",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          apiFormat: "openai_chat",
          claudeDesktopModelRoutes: {
            [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.opus]: {
              model: "deepseek-pro",
              labelOverride: "Pro",
            },
          },
        },
      });
      expect(state.baseUrl).toBe("https://relay.example.com");
      expect(state.apiKey).toBe("sk-2");
      expect(state.apiKeyField).toBe("ANTHROPIC_API_KEY");
      expect(state.apiFormat).toBe("openai_chat");
      expect(state.hasSavedRoutes).toBe(true);
      expect(state.routes.map((r) => [r.route, r.model])).toEqual([
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet, ""],
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.opus, "deepseek-pro"],
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.fable, ""],
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.haiku, ""],
      ]);
    });

    it("replaces unsafe saved route ids and keeps the old id as label", () => {
      const state = claudeDesktopFormFromProvider({
        meta: {
          claudeDesktopMode: "direct",
          claudeDesktopModelRoutes: { "gpt-5": { model: "gpt-5" } },
        },
      });
      expect(state.routes).toHaveLength(1);
      expect(isClaudeSafeRoute(state.routes[0].route)).toBe(true);
      expect(state.routes[0].labelOverride).toBe("gpt-5");
    });

    it("envString ignores non-string values", () => {
      expect(envString({ env: { A: 1, B: "b" } }, "A")).toBe("");
      expect(envString({ env: { A: 1, B: "b" } }, "B")).toBe("b");
      expect(envString(undefined, "B")).toBe("");
    });
  });

  describe("toSettingsConfig", () => {
    it("keeps an empty env placeholder for official providers", () => {
      const saved = toClaudeDesktopOfficialSave({
        settingsConfig: { env: { ANTHROPIC_BASE_URL: "x" }, other: 1 },
        meta: {
          claudeDesktopMode: "proxy",
          apiFormat: "openai_chat",
          endpointAutoSelect: true,
          isPartner: true,
        },
      });
      expect(JSON.parse(saved.settingsConfig)).toEqual({ env: {}, other: 1 });
      expect(saved.meta).toEqual({ isPartner: true });
    });

    it("writes the endpoint and key, and direct-mode routes map to themselves", () => {
      const result = toClaudeDesktopSave(
        saveInput({
          initialData: {
            settingsConfig: { env: { ANTHROPIC_API_KEY: "old", KEEP: "1" } },
          },
          routes: [row(" claude-sonnet-4-6 ", "", "", true)],
        }),
      );
      expect(result.ok).toBe(true);
      if (!result.ok) return;
      expect(JSON.parse(result.settingsConfig)).toEqual({
        env: {
          KEEP: "1",
          ANTHROPIC_BASE_URL: "https://relay.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-1",
        },
      });
      expect(result.meta).toMatchObject({
        claudeDesktopMode: "direct",
        apiFormat: "anthropic",
        claudeDesktopModelRoutes: {
          "claude-sonnet-4-6": {
            model: "claude-sonnet-4-6",
            supports1m: true,
          },
        },
      });
    });

    it("fills empty proxy roles from the first filled one", () => {
      const result = toClaudeDesktopSave(
        saveInput({
          mode: "proxy",
          apiFormat: "openai_chat",
          routes: [
            row(CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet, "ds-pro", "", true),
            row(CLAUDE_DESKTOP_ROLE_ROUTE_IDS.haiku),
          ],
        }),
      );
      expect(result.ok).toBe(true);
      if (!result.ok) return;
      expect(result.meta.apiFormat).toBe("openai_chat");
      expect(result.meta.claudeDesktopModelRoutes).toEqual({
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet]: {
          model: "ds-pro",
          labelOverride: "ds-pro",
          supports1m: true,
        },
        [CLAUDE_DESKTOP_ROLE_ROUTE_IDS.haiku]: {
          model: "ds-pro",
          labelOverride: "ds-pro",
          supports1m: true,
        },
      });
    });

    it("rejects proxy mode without any model and unsafe direct ids", () => {
      expect(
        toClaudeDesktopSave(
          saveInput({
            mode: "proxy",
            routes: [row(CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet)],
          }),
        ),
      ).toEqual({ ok: false, error: "claudeDesktop.routesRequired" });
      expect(
        toClaudeDesktopSave(saveInput({ routes: [row("claude-sonnet-")] })),
      ).toEqual({ ok: false, error: "claudeDesktop.directModelInvalid" });
    });

    it("binds a managed account instead of writing an API key", () => {
      const result = toClaudeDesktopSave(
        saveInput({
          usesManagedOAuth: true,
          providerType: "codex_oauth",
          codexAccountId: "acc-1",
          codexFastMode: true,
        }),
      );
      expect(result.ok).toBe(true);
      if (!result.ok) return;
      expect(JSON.parse(result.settingsConfig).env).toEqual({
        ANTHROPIC_BASE_URL: "https://relay.example.com",
      });
      expect(result.meta).toMatchObject({
        providerType: "codex_oauth",
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acc-1",
        },
        codexFastMode: true,
      });
    });
  });

  it("loads the presets lazily", async () => {
    const presets = await claudeDesktopPresetLoader.load();
    expect(claudeDesktopPresetEntries(presets)[0].id).toBe("claude-desktop-0");
  });
});
