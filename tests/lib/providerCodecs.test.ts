import { describe, expect, it } from "vitest";
import {
  getProviderCodec,
  getProviderEndpoint,
  getProviderModels,
} from "@/lib/providerCodecs";
import { ALL_APP_IDS } from "@/config/apps";
import { buildGrokBuildConfig } from "@/utils/grokBuildConfig";

const CODEX_TOML = `model_provider = "relay"
model = "gpt-5"

[model_providers.relay]
name = "relay"
base_url = "https://codex.example.com/v1"
wire_api = "responses"
`;

describe("provider codecs (FE-PROV-22)", () => {
  it("has a codec with a parsable default config for every app", () => {
    for (const appId of ALL_APP_IDS) {
      const codec = getProviderCodec(appId);
      expect(codec.appId).toBe(appId);
      expect(() => JSON.parse(codec.defaultConfig)).not.toThrow();
    }
  });

  it("claude: reads env endpoint, auth token / api key and models", () => {
    const codec = getProviderCodec("claude");
    const config = {
      env: {
        ANTHROPIC_BASE_URL: "https://relay.example.com",
        ANTHROPIC_API_KEY: "sk-a",
        ANTHROPIC_MODEL: "m1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "m2",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "m1",
      },
    };
    expect(codec.getEndpoint(config)).toBe("https://relay.example.com");
    expect(codec.getApiKey(config)).toBe("sk-a");
    expect(codec.getModels(config)).toEqual(["m1", "m2"]);
    expect(codec.isOfficial(config)).toBe(false);
    expect(codec.isOfficial({ env: {} })).toBe(true);
    // Bedrock-style top-level key wins, template placeholders do not.
    expect(codec.getApiKey({ apiKey: "bedrock", env: {} })).toBe("bedrock");
    expect(
      codec.getApiKey({ apiKey: "${KEY}", env: { ANTHROPIC_AUTH_TOKEN: "t" } }),
    ).toBe("t");
  });

  it("claude: setApiKey keeps the existing field and only creates when asked", () => {
    const codec = getProviderCodec("claude");
    const withApiKey = { env: { ANTHROPIC_API_KEY: "old" } };
    expect(codec.setApiKey(withApiKey, "new")).toEqual({
      env: { ANTHROPIC_API_KEY: "new" },
    });
    expect(withApiKey.env.ANTHROPIC_API_KEY).toBe("old");
    const empty = { env: {} };
    expect(codec.setApiKey(empty, "k")).toBe(empty);
    expect(
      codec.setApiKey(empty, "k", {
        createIfMissing: true,
        apiKeyField: "ANTHROPIC_API_KEY",
      }),
    ).toEqual({ env: { ANTHROPIC_API_KEY: "k" } });
  });

  it("codex: reads the key from auth.OPENAI_API_KEY, not env.CODEX_API_KEY", () => {
    const codec = getProviderCodec("codex");
    const config = {
      auth: { OPENAI_API_KEY: "sk-codex" },
      config: CODEX_TOML,
      modelCatalog: { models: [{ model: "gpt-5" }, { model: "gpt-mini" }] },
    };
    expect(codec.getApiKey(config)).toBe("sk-codex");
    expect(codec.getApiKey({ env: { CODEX_API_KEY: "wrong" } })).toBe("");
    expect(codec.hasApiKeyField(config)).toBe(true);
    expect(codec.getEndpoint(config)).toBe("https://codex.example.com/v1");
    expect(codec.getModels(config)).toEqual(["gpt-5", "gpt-mini"]);
    expect(codec.setApiKey(config, "sk-new").auth).toEqual({
      OPENAI_API_KEY: "sk-new",
    });
    expect(codec.isOfficial({ auth: {}, config: "" })).toBe(true);
    expect(codec.isOfficial(config)).toBe(false);
  });

  it("gemini: env endpoint, key and model", () => {
    const codec = getProviderCodec("gemini");
    const config = {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://g.example.com",
        GEMINI_API_KEY: "g",
        GEMINI_MODEL: "gemini-x",
      },
    };
    expect(codec.getEndpoint(config)).toBe("https://g.example.com");
    expect(codec.getApiKey(config)).toBe("g");
    expect(codec.getModels(config)).toEqual(["gemini-x"]);
    expect(codec.isOfficial({ env: {} })).toBe(true);
  });

  it("opencode / openclaw / hermes / grokbuild: endpoints and models", () => {
    expect(
      getProviderEndpoint("opencode", {
        settingsConfig: { options: { baseURL: "https://oc.example.com" } },
      }),
    ).toBe("https://oc.example.com");
    expect(
      getProviderModels("opencode", {
        settingsConfig: { models: { a: {}, b: {} } },
      }),
    ).toEqual(["a", "b"]);
    expect(
      getProviderEndpoint("openclaw", {
        settingsConfig: { baseUrl: "https://ocl.example.com" },
      }),
    ).toBe("https://ocl.example.com");
    expect(
      getProviderModels("openclaw", {
        settingsConfig: { models: [{ id: "m1" }, { id: "m2" }] },
      }),
    ).toEqual(["m1", "m2"]);
    expect(
      getProviderEndpoint("hermes", {
        settingsConfig: { base_url: "https://h.example.com" },
      }),
    ).toBe("https://h.example.com");
    const grokToml = buildGrokBuildConfig({
      model: "grok-4.5",
      upstreamModel: "relay-model",
      baseUrl: "https://grok.example.com/v1",
      name: "relay",
      apiKey: "xai",
      apiBackend: "responses",
      contextWindow: 1000,
    });
    expect(
      getProviderEndpoint("grokbuild", {
        settingsConfig: { config: grokToml },
      }),
    ).toBe("https://grok.example.com/v1");
    expect(getProviderCodec("grokbuild").getApiKey({ config: grokToml })).toBe(
      "xai",
    );
  });

  it("opencode setApiKey creates the nested options object only on request", () => {
    const codec = getProviderCodec("opencode");
    const bare = { npm: "x" };
    expect(codec.setApiKey(bare, "k")).toBe(bare);
    expect(codec.setApiKey(bare, "k", { createIfMissing: true })).toEqual({
      npm: "x",
      options: { apiKey: "k" },
    });
  });
});
