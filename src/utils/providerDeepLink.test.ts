import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  buildProviderDeepLink,
  encodeDeepLinkConfig,
  isShareableApp,
} from "./providerDeepLink";

const decodeConfig = (url: string) => {
  const params = new URL(url).searchParams;
  const b64 = params.get("config");
  expect(b64).toBeTruthy();
  const binary = atob(b64!);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return {
    params,
    config: JSON.parse(new TextDecoder().decode(bytes)) as Record<string, any>,
  };
};

const claudeProvider: Provider = {
  id: "p1",
  name: "Relay 中文",
  websiteUrl: "https://relay.example",
  icon: "anthropic",
  notes: "team key",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.relay.example/v1",
      ANTHROPIC_AUTH_TOKEN: "sk-secret",
      ANTHROPIC_MODEL: "claude-sonnet-4-5",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5",
      ANTHROPIC_CUSTOM_HEADERS: "x-team: a",
    },
  },
  meta: {
    custom_endpoints: {
      "https://api.relay.example/v1": {
        url: "https://api.relay.example/v1",
        addedAt: 1,
      },
      "https://backup.relay.example/v1/": {
        url: "https://backup.relay.example/v1/",
        addedAt: 2,
      },
    },
  },
};

describe("providerDeepLink", () => {
  it("knows which apps can be shared", () => {
    expect(isShareableApp("claude")).toBe(true);
    expect(isShareableApp("grokbuild")).toBe(true);
    expect(isShareableApp("claude-desktop")).toBe(false);
    expect(isShareableApp("hermes")).toBe(false);
  });

  it("encodes UTF-8 config payloads the backend can decode", () => {
    expect(encodeDeepLinkConfig('{"a":"中"}')).toBe(
      btoa(String.fromCharCode(...new TextEncoder().encode('{"a":"中"}'))),
    );
  });

  it("builds a Claude link without the key and keeps custom env in the config", () => {
    const url = buildProviderDeepLink(claudeProvider, "claude", {
      includeApiKey: false,
    });
    expect(
      url.startsWith("ccswitch://v1/import?resource=provider&app=claude"),
    ).toBe(true);
    const { params, config } = decodeConfig(url);

    expect(params.get("name")).toBe("Relay 中文");
    expect(params.get("endpoint")).toBe(
      "https://api.relay.example/v1,https://backup.relay.example/v1",
    );
    expect(params.get("apiKey")).toBeNull();
    expect(params.get("model")).toBe("claude-sonnet-4-5");
    expect(params.get("haikuModel")).toBe("claude-haiku-4-5");
    expect(params.get("homepage")).toBe("https://relay.example");
    expect(params.get("icon")).toBe("anthropic");
    expect(params.get("notes")).toBe("team key");
    expect(params.get("configFormat")).toBe("json");
    expect(config.env.ANTHROPIC_AUTH_TOKEN).toBeUndefined();
    expect(config.env.ANTHROPIC_API_KEY).toBeUndefined();
    expect(config.env.ANTHROPIC_CUSTOM_HEADERS).toBe("x-team: a");
    expect(url).not.toContain("sk-secret");
  });

  it("includes the Claude key when asked", () => {
    const url = buildProviderDeepLink(claudeProvider, "claude", {
      includeApiKey: true,
    });
    const { params, config } = decodeConfig(url);
    expect(params.get("apiKey")).toBe("sk-secret");
    expect(config.env.ANTHROPIC_AUTH_TOKEN).toBe("sk-secret");
  });

  it("derives Codex parameters and strips the bearer token from the TOML", () => {
    const provider: Provider = {
      id: "c1",
      name: "Codex Relay",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-codex" },
        config: `model = "gpt-5"
model_provider = "relay"

[model_providers.relay]
name = "Relay"
base_url = "https://codex.relay.example/v1"
wire_api = "responses"
experimental_bearer_token = "sk-codex"
`,
      },
    };

    const url = buildProviderDeepLink(provider, "codex", {
      includeApiKey: false,
    });
    const { params, config } = decodeConfig(url);
    expect(params.get("app")).toBe("codex");
    expect(params.get("endpoint")).toBe("https://codex.relay.example/v1");
    expect(params.get("model")).toBe("gpt-5");
    expect(params.get("apiKey")).toBeNull();
    expect(config.auth.OPENAI_API_KEY).toBeUndefined();
    expect(config.config).toContain(
      'base_url = "https://codex.relay.example/v1"',
    );
    expect(config.config).not.toContain("experimental_bearer_token");
    expect(url).not.toContain("sk-codex");

    const withKey = decodeConfig(
      buildProviderDeepLink(provider, "codex", { includeApiKey: true }),
    );
    expect(withKey.params.get("apiKey")).toBe("sk-codex");
    expect(withKey.config.config).toContain("experimental_bearer_token");
  });

  it("derives Grok Build parameters and strips api_key but keeps env_key", () => {
    const provider: Provider = {
      id: "g1",
      name: "Grok Relay",
      settingsConfig: {
        config: `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "upstream-grok"
base_url = "https://grok.relay.example/v1"
name = "Grok Relay"
api_key = "grok-secret"
env_key = "XAI_API_KEY"
api_backend = "responses"
context_window = 500000
`,
      },
    };

    const url = buildProviderDeepLink(provider, "grokbuild", {
      includeApiKey: false,
    });
    const { params, config } = decodeConfig(url);
    expect(params.get("endpoint")).toBe("https://grok.relay.example/v1");
    expect(params.get("model")).toBe("upstream-grok");
    expect(config.config).not.toContain("grok-secret");
    expect(config.config).toContain('env_key = "XAI_API_KEY"');
  });

  it("derives Gemini parameters", () => {
    const provider: Provider = {
      id: "ge1",
      name: "Gemini Relay",
      settingsConfig: {
        env: {
          GOOGLE_GEMINI_BASE_URL: "https://gemini.relay.example",
          GEMINI_API_KEY: "gm-secret",
        },
      },
    };
    const url = buildProviderDeepLink(provider, "gemini", {
      includeApiKey: false,
    });
    const { params, config } = decodeConfig(url);
    expect(params.get("endpoint")).toBe("https://gemini.relay.example");
    // Flat payload: the importer reads GEMINI_* keys from the top level.
    expect(config.env).toBeUndefined();
    expect(config.GOOGLE_GEMINI_BASE_URL).toBe("https://gemini.relay.example");
    expect(config.GEMINI_API_KEY).toBeUndefined();
    expect(url).not.toContain("gm-secret");
  });
});
