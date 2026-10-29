import { describe, expect, it } from "vitest";
import { grokBuildProviderFormSchema } from "@/features/providers/forms/grokbuild/schema";
import {
  grokApiBackendFromApiFormat,
  grokBuildFormFromProvider,
} from "@/features/providers/forms/grokbuild/fromProvider";
import {
  toGrokBuildSave,
  type GrokBuildSaveInput,
} from "@/features/providers/forms/grokbuild/toSettingsConfig";
import { grokBuildPresetEntries } from "@/features/providers/forms/grokbuild/presets";
import { codexPresetLoader } from "@/features/providers/forms/codex/presets";
import { parseGrokBuildConfig } from "@/utils/grokBuildConfig";

const CONFIG = `[models]
default = "grok-relay"

[model.grok-relay]
model = "relay-large"
base_url = "https://relay.example.com/v1"
name = "Relay"
api_key = "sk-1"
api_backend = "chat_completions"
context_window = 128000
`;

const saveInput = (
  overrides: Partial<GrokBuildSaveInput>,
): GrokBuildSaveInput => {
  const state = grokBuildFormFromProvider({
    settingsConfig: { config: CONFIG },
  });
  return {
    name: "Relay",
    rawConfig: state.rawConfig,
    profile: state.profile,
    upstreamModel: state.upstreamModel,
    baseUrl: state.baseUrl,
    apiKey: state.apiKey,
    apiBackend: state.apiBackend,
    contextWindow: state.contextWindow,
    apiFormat: "openai_chat",
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    isFullUrl: false,
    endpointAutoSelect: true,
    isPartner: false,
    impersonateClaudeCode: false,
    promptCacheRouting: "auto",
    codexChatReasoning: {},
    customUserAgent: "",
    headersOverride: "",
    bodyOverride: "",
    maxOutputTokens: "",
    draftCustomEndpoints: [],
    ...overrides,
  };
};

describe("grokbuild form module (CCS-191)", () => {
  it("schema is the grokbuild branch of the union", () => {
    expect(
      grokBuildProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "grokbuild",
      }).success,
    ).toBe(true);
  });

  describe("fromProvider", () => {
    it("builds a default config.toml for a new provider", () => {
      const state = grokBuildFormFromProvider(undefined);
      expect(state.category).toBe("custom");
      expect(state.profile).toBe("grok-4.5");
      expect(state.apiFormat).toBe("openai_responses");
      expect(parseGrokBuildConfig(state.rawConfig).model).toBe("grok-4.5");
      expect(JSON.parse(state.defaultValues.settingsConfig)).toEqual({
        config: state.rawConfig,
      });
    });

    it("reads the structured fields from the saved config.toml", () => {
      const state = grokBuildFormFromProvider({
        settingsConfig: { config: CONFIG },
        meta: { apiFormat: "openai_chat", maxOutputTokens: 4096 },
      });
      expect(state).toMatchObject({
        configName: "Relay",
        profile: "grok-relay",
        upstreamModel: "relay-large",
        baseUrl: "https://relay.example.com/v1",
        apiKey: "sk-1",
        apiBackend: "chat_completions",
        contextWindow: "128000",
        rawConfig: CONFIG,
        apiFormat: "openai_chat",
        maxOutputTokens: "4096",
      });
      expect(state.defaultValues.name).toBe("Relay");
    });

    it("maps the API format to Grok Build's api_backend", () => {
      expect(grokApiBackendFromApiFormat("openai_chat")).toBe(
        "chat_completions",
      );
      expect(grokApiBackendFromApiFormat("anthropic")).toBe("messages");
      expect(grokApiBackendFromApiFormat("openai_responses")).toBe("responses");
    });
  });

  describe("toSettingsConfig", () => {
    it("writes the structured fields back into config.toml and meta", () => {
      const result = toGrokBuildSave(
        saveInput({
          baseUrl: "https://relay2.example.com/v1",
          customUserAgent: " ua ",
          maxOutputTokens: "8192",
          headersOverride: '{"x-a":"1"}',
          draftCustomEndpoints: ["https://alt.example.com"],
          now: 1,
        }),
      );
      expect(result.ok).toBe(true);
      if (!result.ok) return;
      const { config } = JSON.parse(result.settingsConfig);
      expect(parseGrokBuildConfig(config)).toMatchObject({
        model: "grok-relay",
        baseUrl: "https://relay2.example.com/v1",
        contextWindow: 128000,
      });
      expect(result.meta).toMatchObject({
        apiFormat: "openai_chat",
        customUserAgent: "ua",
        maxOutputTokens: 8192,
        localProxyRequestOverrides: { headers: { "x-a": "1" } },
        custom_endpoints: {
          "https://alt.example.com": {
            url: "https://alt.example.com",
            addedAt: 1,
          },
        },
      });
    });

    it("does not add draft endpoints when editing an existing provider", () => {
      const result = toGrokBuildSave(
        saveInput({
          providerId: "p1",
          initialMeta: {
            custom_endpoints: { a: { url: "a", addedAt: 0 } },
          },
          draftCustomEndpoints: ["https://alt.example.com"],
        }),
      );
      expect(result.ok && result.meta.custom_endpoints).toBeUndefined();
    });

    it("rejects missing fields, a bad context window and bad overrides", () => {
      expect(toGrokBuildSave(saveInput({ baseUrl: " " }))).toEqual({
        ok: false,
        error: { key: "providerForm.requiredFields" },
      });
      expect(toGrokBuildSave(saveInput({ contextWindow: "0" }))).toEqual({
        ok: false,
        error: { key: "grokBuild.contextWindowInvalid" },
      });
      const overrides = toGrokBuildSave(saveInput({ bodyOverride: "{" }));
      expect(overrides.ok).toBe(false);
      if (overrides.ok) return;
      expect(overrides.error.key).toBeNull();
    });
  });

  it("offers only the non-official Codex presets, numbered by Codex index", async () => {
    const presets = await codexPresetLoader.load();
    const entries = grokBuildPresetEntries(presets);
    expect(entries.length).toBeGreaterThan(0);
    for (const entry of entries) {
      expect(entry.preset.category).not.toBe("official");
      expect(entry.preset.isOfficial).not.toBe(true);
      expect(presets[Number(entry.id.replace("grokbuild-", ""))]).toBe(
        entry.preset,
      );
    }
  });
});
