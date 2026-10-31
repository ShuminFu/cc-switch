/** Default `settingsConfig` text per app for a new provider. */

export const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);
export const CLAUDE_DESKTOP_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "",
      ANTHROPIC_AUTH_TOKEN: "",
    },
  },
  null,
  2,
);
export const CODEX_DEFAULT_CONFIG = JSON.stringify(
  { auth: {}, config: "" },
  null,
  2,
);
export const GEMINI_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      GOOGLE_GEMINI_BASE_URL: "",
      GEMINI_API_KEY: "",
      GEMINI_MODEL: "gemini-3.5-flash",
    },
  },
  null,
  2,
);

export const OPENCODE_DEFAULT_NPM = "@ai-sdk/openai-compatible";
export const OPENCODE_DEFAULT_CONFIG = JSON.stringify(
  {
    npm: OPENCODE_DEFAULT_NPM,
    options: {
      baseURL: "",
      apiKey: "",
      setCacheKey: true,
    },
    models: {},
  },
  null,
  2,
);
export const OPENCLAW_DEFAULT_CONFIG = JSON.stringify(
  {
    baseUrl: "",
    apiKey: "",
    api: "openai-completions",
    models: [],
  },
  null,
  2,
);
