import type { AppId } from "@/lib/api";
import { getAppDefinition, type ProviderFormAdapterId } from "@/config/apps";
import { ClaudeDesktopProviderForm } from "../ClaudeDesktopProviderForm";
import { GrokBuildProviderForm } from "../GrokBuildProviderForm";
import { claudeFormAdapter } from "./claude";
import { codexFormAdapter } from "./codex";
import { geminiFormAdapter } from "./gemini";
import { hermesFormAdapter } from "./hermes";
import { openclawFormAdapter } from "./openclaw";
import { opencodeFormAdapter } from "./opencode";
import type { ProviderFormAdapter } from "./types";

/**
 * Provider form adapter implementations, keyed by the adapter id each app
 * declares in the app registry (`APP_REGISTRY[appId].providerForm`).
 * TypeScript requires exactly one implementation per adapter id.
 */
export const PROVIDER_FORM_ADAPTERS: Readonly<
  Record<ProviderFormAdapterId, ProviderFormAdapter>
> = {
  claude: claudeFormAdapter,
  claudeDesktop: { kind: "standalone", Form: ClaudeDesktopProviderForm },
  codex: codexFormAdapter,
  gemini: geminiFormAdapter,
  grokbuild: { kind: "standalone", Form: GrokBuildProviderForm },
  opencode: opencodeFormAdapter,
  openclaw: openclawFormAdapter,
  hermes: hermesFormAdapter,
};

export const getProviderFormAdapter = (appId: AppId): ProviderFormAdapter =>
  PROVIDER_FORM_ADAPTERS[getAppDefinition(appId).providerForm];

export type * from "./types";
