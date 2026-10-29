import { describe, expect, it } from "vitest";
import { CLAUDE_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import { claudeProviderFormSchema } from "@/features/providers/forms/claude/schema";
import {
  claudeApiKeyFieldFromProvider,
  claudeFormFromProvider,
} from "@/features/providers/forms/claude/fromProvider";
import {
  toClaudeAppMeta,
  toClaudeSettingsConfig,
} from "@/features/providers/forms/claude/toSettingsConfig";
import {
  claudePresetEntries,
  claudePresetLoader,
} from "@/features/providers/forms/claude/presets";

const values = {
  name: "Relay",
  websiteUrl: "",
  notes: "",
  settingsConfig: '  {"env":{}}\n',
  icon: "",
  iconColor: "",
};

describe("claude form module (CCS-191)", () => {
  it("schema is the claude branch of the union", () => {
    expect(
      claudeProviderFormSchema.safeParse({ ...values, appId: "claude" })
        .success,
    ).toBe(true);
    expect(
      claudeProviderFormSchema.safeParse({ ...values, appId: "codex" }).success,
    ).toBe(false);
  });

  describe("fromProvider", () => {
    it("uses the default settings.json for a new provider", () => {
      const state = claudeFormFromProvider(undefined);
      expect(state.defaultValues).toEqual({
        name: "",
        websiteUrl: "",
        notes: "",
        settingsConfig: CLAUDE_DEFAULT_CONFIG,
        icon: "",
        iconColor: "",
      });
      expect(state.apiKeyField).toBe("ANTHROPIC_AUTH_TOKEN");
      expect(state.apiFormat).toBe("anthropic");
      expect(state.isFullUrl).toBe(false);
      expect(state.endpointAutoSelect).toBe(true);
      expect(state.pricing).toEqual({
        enabled: false,
        costMultiplier: undefined,
        pricingModelSource: "inherit",
      });
    });

    it("reads a saved provider back into the editor state", () => {
      const state = claudeFormFromProvider({
        name: "Relay",
        settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://r.example" } },
        meta: {
          apiFormat: "openai_chat",
          isFullUrl: true,
          endpointAutoSelect: false,
          costMultiplier: "1.5",
          customUserAgent: "ua/1",
          localProxyRequestOverrides: { headers: { "x-a": "1" } },
          authBinding: {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId: "gh-1",
          },
        },
      });
      expect(JSON.parse(state.defaultValues.settingsConfig)).toEqual({
        env: { ANTHROPIC_BASE_URL: "https://r.example" },
      });
      expect(state.apiFormat).toBe("openai_chat");
      expect(state.isFullUrl).toBe(true);
      expect(state.endpointAutoSelect).toBe(false);
      expect(state.pricing.enabled).toBe(true);
      expect(state.pricing.costMultiplier).toBe("1.5");
      expect(state.customUserAgent).toBe("ua/1");
      expect(JSON.parse(state.localProxyHeadersOverride)).toEqual({
        "x-a": "1",
      });
      expect(state.githubAccountId).toBe("gh-1");
    });

    it("infers the API key field from the env when meta does not say", () => {
      expect(
        claudeApiKeyFieldFromProvider({
          settingsConfig: { env: { ANTHROPIC_API_KEY: "" } },
        }),
      ).toBe("ANTHROPIC_API_KEY");
      expect(
        claudeApiKeyFieldFromProvider({
          settingsConfig: { env: { ANTHROPIC_API_KEY: "" } },
          meta: { apiKeyField: "ANTHROPIC_AUTH_TOKEN" },
        }),
      ).toBe("ANTHROPIC_AUTH_TOKEN");
      expect(claudeApiKeyFieldFromProvider({ settingsConfig: {} })).toBe(
        "ANTHROPIC_AUTH_TOKEN",
      );
    });
  });

  describe("toSettingsConfig", () => {
    it("saves the edited settings.json trimmed", () => {
      expect(toClaudeSettingsConfig(values)).toBe('{"env":{}}');
    });

    it("writes the Claude meta fields for a third-party provider", () => {
      expect(
        toClaudeAppMeta({
          category: "third_party",
          commonConfigEnabled: true,
          apiFormat: "openai_chat",
          apiKeyField: "ANTHROPIC_API_KEY",
          isFullUrl: true,
          customUserAgent: "  ua/1 ",
          localProxyRequestOverrides: { headers: { "x-a": "1" } },
        }),
      ).toEqual({
        commonConfigEnabled: true,
        codexChatReasoning: undefined,
        promptCacheRouting: undefined,
        customUserAgent: "ua/1",
        localProxyRequestOverrides: { headers: { "x-a": "1" } },
        apiFormat: "openai_chat",
        apiKeyField: "ANTHROPIC_API_KEY",
        impersonateClaudeCode: undefined,
        maxOutputTokens: undefined,
        isFullUrl: true,
      });
    });

    it("drops proxy-only meta for official providers and the default key field", () => {
      const meta = toClaudeAppMeta({
        category: "official",
        commonConfigEnabled: false,
        apiFormat: "openai_chat",
        apiKeyField: "ANTHROPIC_API_KEY",
        isFullUrl: true,
        customUserAgent: "ua/1",
        localProxyRequestOverrides: { headers: { "x-a": "1" } },
      });
      expect(meta.apiFormat).toBeUndefined();
      expect(meta.apiKeyField).toBeUndefined();
      expect(meta.isFullUrl).toBeUndefined();
      expect(meta.customUserAgent).toBeUndefined();
      expect(meta.localProxyRequestOverrides).toBeUndefined();

      expect(
        toClaudeAppMeta({
          category: "custom",
          commonConfigEnabled: false,
          apiFormat: "anthropic",
          apiKeyField: "ANTHROPIC_AUTH_TOKEN",
          isFullUrl: false,
          customUserAgent: " ",
        }),
      ).toMatchObject({
        apiFormat: "anthropic",
        apiKeyField: undefined,
        isFullUrl: undefined,
        customUserAgent: undefined,
      });
    });
  });

  it("loads the presets lazily and numbers entries after hiding", async () => {
    const presets = await claudePresetLoader.load();
    expect(claudePresetLoader.peek()).toBe(presets);
    const entries = claudePresetEntries([
      { ...presets[0], hidden: true },
      presets[1],
    ]);
    expect(entries).toEqual([{ id: "claude-0", preset: presets[1] }]);
  });
});
