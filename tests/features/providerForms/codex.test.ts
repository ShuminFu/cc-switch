import { describe, expect, it } from "vitest";
import { CODEX_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import { codexProviderFormSchema } from "@/features/providers/forms/codex/schema";
import {
  codexApiFormatFromProvider,
  codexFormFromProvider,
} from "@/features/providers/forms/codex/fromProvider";
import {
  toCodexAppMeta,
  toCodexSettingsConfig,
  type CodexMetaInput,
} from "@/features/providers/forms/codex/toSettingsConfig";
import {
  codexPresetEntries,
  codexPresetLoader,
} from "@/features/providers/forms/codex/presets";
import { extractCodexModelName } from "@/utils/providerConfigUtils";

const CONFIG = `model_provider = "relay"
model = "gpt-5"

[model_providers.relay]
name = "relay"
base_url = "https://relay.example.com/v1"
wire_api = "chat"
`;

const metaInput = (overrides: Partial<CodexMetaInput>): CodexMetaInput => ({
  category: "third_party",
  commonConfigEnabled: false,
  apiFormat: "openai_responses",
  anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
  impersonateClaudeCode: false,
  maxOutputTokens: "",
  codexChatReasoning: {},
  promptCacheRouting: "auto",
  isFullUrl: false,
  customUserAgent: "",
  ...overrides,
});

describe("codex form module (CCS-191)", () => {
  it("schema is the codex branch of the union", () => {
    expect(
      codexProviderFormSchema.safeParse({
        name: "x",
        settingsConfig: "{}",
        appId: "codex",
      }).success,
    ).toBe(true);
  });

  describe("fromProvider", () => {
    it("uses the default auth/config for a new provider", () => {
      const state = codexFormFromProvider(undefined);
      expect(state.defaultValues.settingsConfig).toBe(CODEX_DEFAULT_CONFIG);
      expect(state.apiFormat).toBe("openai_responses");
      expect(state.anthropicAuthField).toBe("ANTHROPIC_AUTH_TOKEN");
      expect(state.impersonateClaudeCode).toBe(false);
      expect(state.maxOutputTokens).toBe("");
      expect(state.codexChatReasoning).toEqual({});
      expect(state.promptCacheRouting).toBe("auto");
    });

    it("reads the Anthropic upstream options back", () => {
      const state = codexFormFromProvider({
        settingsConfig: { auth: {}, config: CONFIG },
        meta: {
          apiFormat: "anthropic",
          apiKeyField: "ANTHROPIC_API_KEY",
          impersonateClaudeCode: true,
          maxOutputTokens: 32000,
          promptCacheRouting: "enabled",
        },
      });
      expect(state.apiFormat).toBe("anthropic");
      expect(state.anthropicAuthField).toBe("ANTHROPIC_API_KEY");
      expect(state.impersonateClaudeCode).toBe(true);
      expect(state.maxOutputTokens).toBe("32000");
      expect(state.promptCacheRouting).toBe("enabled");
    });

    it("infers the API format from wire_api when meta has none", () => {
      expect(
        codexApiFormatFromProvider({
          settingsConfig: { config: CONFIG },
        }),
      ).toBe("openai_chat");
      expect(
        codexApiFormatFromProvider({
          settingsConfig: { config: CONFIG },
          meta: { apiFormat: "gemini_native" },
        }),
      ).toBe("openai_chat");
      expect(codexApiFormatFromProvider(undefined)).toBe("openai_responses");
    });
  });

  describe("toSettingsConfig", () => {
    it("pins wire_api to responses and saves the model catalog", () => {
      const saved = JSON.parse(
        toCodexSettingsConfig({
          auth: { OPENAI_API_KEY: "sk" },
          config: CONFIG.replace('model = "gpt-5"\n', ""),
          catalogModels: [
            { model: " relay-large ", displayName: " Large " },
            { model: "relay-large" },
            { model: "" },
          ],
          category: "third_party",
        }),
      );
      expect(saved.auth).toEqual({ OPENAI_API_KEY: "sk" });
      expect(saved.config).toContain('wire_api = "responses"');
      // Default model falls back to the first catalog row when left empty.
      expect(extractCodexModelName(saved.config)).toBe("relay-large");
      expect(saved.modelCatalog).toEqual({
        models: [{ model: "relay-large", displayName: "Large" }],
      });
    });

    it("leaves official providers' config and catalog alone", () => {
      const saved = JSON.parse(
        toCodexSettingsConfig({
          auth: {},
          config: CONFIG,
          catalogModels: [{ model: "relay-large" }],
          category: "official",
        }),
      );
      expect(saved).toEqual({ auth: {}, config: CONFIG });
    });

    it("keeps Chat-only meta for openai_chat and Anthropic-only meta for anthropic", () => {
      const chat = toCodexAppMeta(
        metaInput({
          apiFormat: "openai_chat",
          codexChatReasoning: { supportsThinking: true },
          promptCacheRouting: "disabled",
          impersonateClaudeCode: true,
          maxOutputTokens: "100",
        }),
      );
      expect(chat.codexChatReasoning).toMatchObject({
        supportsThinking: true,
        thinkingParam: "thinking",
      });
      expect(chat.promptCacheRouting).toBe("disabled");
      expect(chat.impersonateClaudeCode).toBeUndefined();
      expect(chat.maxOutputTokens).toBeUndefined();

      const anthropic = toCodexAppMeta(
        metaInput({
          apiFormat: "anthropic",
          anthropicAuthField: "ANTHROPIC_API_KEY",
          impersonateClaudeCode: true,
          maxOutputTokens: " 64000 ",
          promptCacheRouting: "disabled",
        }),
      );
      expect(anthropic).toMatchObject({
        apiFormat: "anthropic",
        apiKeyField: "ANTHROPIC_API_KEY",
        impersonateClaudeCode: true,
        maxOutputTokens: 64000,
        codexChatReasoning: undefined,
        promptCacheRouting: undefined,
      });
      expect(
        toCodexAppMeta(
          metaInput({ apiFormat: "anthropic", maxOutputTokens: "0" }),
        ).maxOutputTokens,
      ).toBeUndefined();
    });

    it("writes no format meta for official providers", () => {
      const meta = toCodexAppMeta(
        metaInput({
          category: "official",
          apiFormat: "anthropic",
          isFullUrl: true,
          customUserAgent: "ua",
          localProxyRequestOverrides: { body: { a: 1 } },
        }),
      );
      expect(meta.apiFormat).toBeUndefined();
      expect(meta.isFullUrl).toBeUndefined();
      expect(meta.customUserAgent).toBeUndefined();
      expect(meta.localProxyRequestOverrides).toBeUndefined();
    });
  });

  it("loads the presets lazily", async () => {
    const presets = await codexPresetLoader.load();
    expect(codexPresetEntries(presets)[1]).toEqual({
      id: "codex-1",
      preset: presets[1],
    });
  });
});
