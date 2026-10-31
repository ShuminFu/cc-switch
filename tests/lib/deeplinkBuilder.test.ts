import { describe, expect, it } from "vitest";
import { buildProviderDeeplink } from "@/lib/deeplinkBuilder";
import type { Provider } from "@/types";

/** Parse a link the way `parse_deeplink_url` does (scheme, host, query). */
function parse(link: string) {
  const url = new URL(link);
  expect(url.protocol).toBe("ccswitch:");
  expect(url.host).toBe("v1");
  expect(url.pathname).toBe("/import");
  return Object.fromEntries(url.searchParams.entries());
}

// FE-PROV-55
describe("buildProviderDeeplink", () => {
  it("round-trips a Claude provider and leaves the key out by default", () => {
    const provider: Provider = {
      id: "p",
      name: "My Relay & Co",
      websiteUrl: "https://relay.example.com",
      notes: "team relay",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.relay.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-secret",
          ANTHROPIC_MODEL: "claude-x",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "haiku-x",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet-x",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "opus-x",
        },
      },
    };
    const params = parse(buildProviderDeeplink(provider, "claude"));
    expect(params).toEqual({
      resource: "provider",
      app: "claude",
      name: "My Relay & Co",
      endpoint: "https://api.relay.example.com",
      homepage: "https://relay.example.com",
      model: "claude-x",
      haikuModel: "haiku-x",
      sonnetModel: "sonnet-x",
      opusModel: "opus-x",
      notes: "team relay",
    });
    expect(
      parse(buildProviderDeeplink(provider, "claude", { includeApiKey: true }))
        .apiKey,
    ).toBe("sk-secret");
  });

  it("round-trips a Codex provider", () => {
    const provider: Provider = {
      id: "c",
      name: "Codex Relay",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-codex" },
        config: `model_provider = "relay"
model = "gpt-5"

[model_providers.relay]
base_url = "https://codex.example.com/v1"
`,
      },
    };
    const params = parse(
      buildProviderDeeplink(provider, "codex", { includeApiKey: true }),
    );
    expect(params).toMatchObject({
      app: "codex",
      name: "Codex Relay",
      endpoint: "https://codex.example.com/v1",
      apiKey: "sk-codex",
      model: "gpt-5",
    });
  });

  it("round-trips a Gemini provider", () => {
    const provider: Provider = {
      id: "g",
      name: "Gemini Relay",
      settingsConfig: {
        env: {
          GOOGLE_GEMINI_BASE_URL: "https://gemini.example.com",
          GEMINI_API_KEY: "g-key",
          GEMINI_MODEL: "gemini-3",
        },
      },
    };
    const params = parse(buildProviderDeeplink(provider, "gemini"));
    expect(params).toMatchObject({
      app: "gemini",
      endpoint: "https://gemini.example.com",
      model: "gemini-3",
    });
    expect(params.apiKey).toBeUndefined();
  });
});
