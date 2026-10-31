import type { AppId } from "@/lib/api/types";

/**
 * Per-app shapes of `Provider.settingsConfig` (FE-ARCH-28).
 *
 * The stored value is untyped JSON (`Record<string, unknown>`); these types
 * describe what each app's adapter writes, and `narrowSettingsConfig` turns
 * a raw value into a discriminated union keyed by app, checking the fields
 * whose type the UI relies on. Unknown keys are kept (`[key: string]: unknown`)
 * so nothing the app does not own is dropped.
 */

type Rec = Record<string, unknown>;

/** Claude / Claude Desktop: the settings.json object. */
export interface ClaudeSettingsConfig {
  env?: Record<string, unknown>;
  [key: string]: unknown;
}

/** Codex: auth.json object plus config.toml text. */
export interface CodexSettingsConfig {
  auth?: Rec;
  config?: string;
  modelCatalog?: { models?: unknown[] };
  [key: string]: unknown;
}

/** Gemini: .env pairs plus settings.json object. */
export interface GeminiSettingsConfig {
  env?: Record<string, unknown>;
  config?: Rec;
  [key: string]: unknown;
}

/** Grok Build: config.toml text. */
export interface GrokBuildSettingsConfig {
  config?: string;
  [key: string]: unknown;
}

/** OpenCode / OpenClaw / Hermes: the provider entry object itself. */
export type AdditiveProviderSettingsConfig = Rec;

export interface SettingsConfigByApp {
  claude: ClaudeSettingsConfig;
  "claude-desktop": ClaudeSettingsConfig;
  codex: CodexSettingsConfig;
  gemini: GeminiSettingsConfig;
  grokbuild: GrokBuildSettingsConfig;
  opencode: AdditiveProviderSettingsConfig;
  openclaw: AdditiveProviderSettingsConfig;
  hermes: AdditiveProviderSettingsConfig;
}

/** `{ app, config }` with `config` typed for `app`. */
export type TypedSettingsConfig = {
  [A in keyof SettingsConfigByApp]: { app: A; config: SettingsConfigByApp[A] };
}[keyof SettingsConfigByApp];

const isRec = (value: unknown): value is Rec =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Copy of `raw` with `key` removed when it does not satisfy `ok`. */
function keepIf(raw: Rec, key: string, ok: (value: unknown) => boolean): Rec {
  if (!(key in raw) || ok(raw[key])) return raw;
  const { [key]: _dropped, ...rest } = raw;
  return rest;
}

/**
 * Narrow a raw `settingsConfig` for `app`. A value that is not an object
 * becomes `{}`; typed fields of the wrong type are left out so readers can
 * trust the declared types.
 */
export function narrowSettingsConfig<A extends AppId>(
  app: A,
  raw: unknown,
): { app: A; config: SettingsConfigByApp[A & keyof SettingsConfigByApp] } {
  let config: Rec = isRec(raw) ? raw : {};
  switch (app) {
    case "claude":
    case "claude-desktop":
      config = keepIf(config, "env", isRec);
      break;
    case "codex":
      config = keepIf(config, "auth", isRec);
      config = keepIf(config, "config", (v) => typeof v === "string");
      config = keepIf(config, "modelCatalog", isRec);
      break;
    case "gemini":
      config = keepIf(config, "env", isRec);
      config = keepIf(config, "config", isRec);
      break;
    case "grokbuild":
      config = keepIf(config, "config", (v) => typeof v === "string");
      break;
    default:
      break;
  }
  return {
    app,
    config: config as SettingsConfigByApp[A & keyof SettingsConfigByApp],
  };
}

/** Shorthand: the typed config of `app` (see `narrowSettingsConfig`). */
export function settingsConfigFor<A extends keyof SettingsConfigByApp>(
  app: A,
  raw: unknown,
): SettingsConfigByApp[A] {
  return narrowSettingsConfig(app, raw).config as SettingsConfigByApp[A];
}
