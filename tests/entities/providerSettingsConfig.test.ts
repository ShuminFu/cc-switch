import { describe, expect, it } from "vitest";
import type { AppId } from "@/lib/api/types";
import { ALL_APP_IDS } from "@/entities/app/registry";
import {
  extractProviderCredentials,
  isOfficialLoginConfig,
  supportsOfficialLogin,
} from "@/entities/provider/settingsConfig";

// 每个应用一份典型 settingsConfig 与期望读出的凭据；新增应用时这里会缺项而失败
const CREDENTIAL_CASES: Record<
  AppId,
  { config: Record<string, unknown>; apiKey: string; baseUrl: string }
> = {
  claude: {
    config: {
      env: {
        ANTHROPIC_AUTH_TOKEN: "sk-claude",
        ANTHROPIC_BASE_URL: "https://claude.example/",
      },
    },
    apiKey: "sk-claude",
    baseUrl: "https://claude.example/",
  },
  "claude-desktop": {
    config: {
      env: {
        ANTHROPIC_API_KEY: "sk-desktop",
        ANTHROPIC_BASE_URL: "https://desktop.example",
      },
    },
    apiKey: "sk-desktop",
    baseUrl: "https://desktop.example",
  },
  codex: {
    config: {
      auth: { OPENAI_API_KEY: "sk-codex" },
      config: [
        'model_provider = "custom"',
        "",
        "[model_providers.custom]",
        'base_url = "https://codex.example/v1"',
      ].join("\n"),
    },
    apiKey: "sk-codex",
    baseUrl: "https://codex.example/v1",
  },
  gemini: {
    config: {
      env: {
        GOOGLE_API_KEY: "sk-gemini",
        GOOGLE_GEMINI_BASE_URL: "https://gemini.example",
      },
    },
    apiKey: "sk-gemini",
    baseUrl: "https://gemini.example",
  },
  grokbuild: {
    config: {
      config: [
        "[models]",
        'default = "grok-build"',
        "",
        '[model."grok-build"]',
        'base_url = "https://grok.example/v1"',
        'api_key = "sk-grok"',
      ].join("\n"),
    },
    apiKey: "sk-grok",
    baseUrl: "https://grok.example/v1",
  },
  opencode: {
    config: {
      options: { apiKey: "sk-opencode", baseURL: "https://opencode.example" },
    },
    apiKey: "sk-opencode",
    baseUrl: "https://opencode.example",
  },
  openclaw: {
    config: { apiKey: "sk-openclaw", baseUrl: "https://openclaw.example" },
    apiKey: "sk-openclaw",
    baseUrl: "https://openclaw.example",
  },
  hermes: {
    config: { api_key: "sk-hermes", base_url: "https://hermes.example" },
    apiKey: "sk-hermes",
    baseUrl: "https://hermes.example",
  },
};

describe("provider settingsConfig accessors", () => {
  it("covers every registered app", () => {
    expect(Object.keys(CREDENTIAL_CASES).sort()).toEqual(
      [...ALL_APP_IDS].sort(),
    );
  });

  it.each(Object.entries(CREDENTIAL_CASES))(
    "reads %s credentials from its own config shape",
    (appId, { config, apiKey, baseUrl }) => {
      expect(extractProviderCredentials(appId as AppId, config)).toEqual({
        apiKey,
        baseUrl,
      });
    },
  );

  it("does not read another app's config shape", () => {
    const claudeConfig = CREDENTIAL_CASES.claude.config;
    expect(extractProviderCredentials("hermes", claudeConfig)).toEqual({
      apiKey: undefined,
      baseUrl: undefined,
    });
    expect(extractProviderCredentials("opencode", {})).toEqual({
      apiKey: undefined,
      baseUrl: undefined,
    });
  });

  it("falls back to the Codex experimental bearer token", () => {
    const { apiKey } = extractProviderCredentials("codex", {
      auth: { OPENAI_API_KEY: "  " },
      config: [
        'model_provider = "custom"',
        "",
        "[model_providers.custom]",
        'base_url = "https://codex.example/v1"',
        'experimental_bearer_token = "bearer-token"',
      ].join("\n"),
    });
    expect(apiKey).toBe("bearer-token");
  });

  it("supports official login only for Claude, Codex and Gemini", () => {
    expect(ALL_APP_IDS.filter(supportsOfficialLogin)).toEqual([
      "claude",
      "codex",
      "gemini",
    ]);
  });

  it("detects official-login configs per app", () => {
    expect(isOfficialLoginConfig("claude", { env: {} })).toBe(true);
    expect(isOfficialLoginConfig("claude", undefined)).toBe(true);
    expect(
      isOfficialLoginConfig("claude", CREDENTIAL_CASES.claude.config),
    ).toBe(false);

    expect(isOfficialLoginConfig("codex", { auth: {}, config: "" })).toBe(true);
    expect(isOfficialLoginConfig("codex", CREDENTIAL_CASES.codex.config)).toBe(
      false,
    );

    expect(
      isOfficialLoginConfig("gemini", { env: { GEMINI_API_KEY: " " } }),
    ).toBe(true);
    expect(
      isOfficialLoginConfig("gemini", {
        env: { GOOGLE_GEMINI_BASE_URL: "https://gemini.example" },
      }),
    ).toBe(false);

    // 不支持官方登录的应用即使配置为空也不算官方
    expect(isOfficialLoginConfig("hermes", {})).toBe(false);
    expect(isOfficialLoginConfig("claude-desktop", { env: {} })).toBe(false);
  });
});
