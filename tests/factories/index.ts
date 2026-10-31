/**
 * Shared test-data factories (TEST-25).
 *
 * Every factory returns a fresh, realistic object (the shape production data
 * has, including `createdAt`/`sortIndex`) and takes `overrides` for the fields
 * a test cares about. When a type gains a required field, update it here once.
 *
 * Timestamps are derived from {@link FIXED_NOW}, never `Date.now()`, so
 * fixtures are deterministic (TEST-10).
 */
import type { AppId } from "@/lib/api/types";
import type {
  McpServer,
  Provider,
  SessionMeta,
  SessionMessage,
  Settings,
} from "@/types";

/** 2026-01-01T00:00:00Z: the clock every fixture is relative to. */
export const FIXED_NOW = Date.UTC(2026, 0, 1);

let sequence = 0;
/** Deterministic id generator; reset between tests via {@link resetFactories}. */
export const nextId = (prefix: string) => `${prefix}-${++sequence}`;
export const resetFactories = () => {
  sequence = 0;
};

/** A minimal but realistic live config per app. */
export function defaultSettingsConfig(app: AppId): Record<string, unknown> {
  switch (app) {
    case "claude":
    case "claude-desktop":
      return {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      };
    case "codex":
      return {
        auth: { OPENAI_API_KEY: "sk-test" },
        config:
          'model_provider = "example"\nmodel = "gpt-5"\n\n[model_providers.example]\nbase_url = "https://api.example.com/v1"\nwire_api = "responses"\n',
      };
    case "gemini":
      return {
        env: {
          GEMINI_API_KEY: "test-key",
          GOOGLE_GEMINI_BASE_URL: "https://generativelanguage.googleapis.com",
        },
      };
    default:
      return {};
  }
}

export interface ProviderFactoryOptions {
  app?: AppId;
  /** Use an empty `settingsConfig` (e.g. official providers with no live env). */
  emptyConfig?: boolean;
}

export function makeProvider(
  overrides: Partial<Provider> = {},
  { app = "claude", emptyConfig = false }: ProviderFactoryOptions = {},
): Provider {
  const id = overrides.id ?? nextId(`${app}-provider`);
  return {
    id,
    name: overrides.name ?? `Provider ${id}`,
    settingsConfig: emptyConfig ? {} : defaultSettingsConfig(app),
    category: "custom",
    sortIndex: 0,
    createdAt: FIXED_NOW,
    ...overrides,
  };
}

/** A `Record<id, Provider>` from a list, preserving the given ids. */
export function providerMap(list: Provider[]): Record<string, Provider> {
  return Object.fromEntries(list.map((p) => [p.id, p]));
}

export function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    showInTray: true,
    minimizeToTrayOnClose: true,
    enableClaudePluginIntegration: false,
    claudeConfigDir: "/default/claude",
    codexConfigDir: "/default/codex",
    language: "zh",
    ...overrides,
  };
}

export function makeMcpServer(overrides: Partial<McpServer> = {}): McpServer {
  const id = overrides.id ?? nextId("mcp");
  return {
    id,
    name: overrides.name ?? id,
    server: { type: "stdio", command: "npx", args: ["-y", `${id}-server`] },
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      grokbuild: false,
    } as McpServer["apps"],
    ...overrides,
  };
}

export function makeSession(overrides: Partial<SessionMeta> = {}): SessionMeta {
  const sessionId = overrides.sessionId ?? nextId("session");
  const providerId = overrides.providerId ?? "codex";
  return {
    providerId,
    sessionId,
    title: `Session ${sessionId}`,
    summary: "summary",
    projectDir: `/mock/${providerId}`,
    createdAt: FIXED_NOW - 2000,
    lastActiveAt: FIXED_NOW - 1000,
    sourcePath: `/mock/${providerId}/${sessionId}.jsonl`,
    resumeCommand: `${providerId} resume ${sessionId}`,
    ...overrides,
  };
}

export function makeSessionMessage(
  overrides: Partial<SessionMessage> = {},
): SessionMessage {
  return { role: "user", content: "hello", ts: FIXED_NOW, ...overrides };
}
