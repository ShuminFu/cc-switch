/**
 * Dumps the TypeScript provider preset catalogs to JSON resources embedded

 * embedded by the shared Rust crate cc-switch-presets (crates/cc-switch-presets/data/*.json).
 *
 * The TS files stay the source of truth until the React UI is removed; run
 * `pnpm presets:dump` after editing them. CI fails when the JSON is stale.
 *
 *   node_modules/.pnpm/node_modules/.bin/vite-node scripts/dump-presets.ts
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { providerPresets } from "@/config/claudeProviderPresets";
import {
  codexProviderPresets,
  generateThirdPartyAuth,
  generateThirdPartyConfig,
} from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import {
  claudeDesktopProviderPresets,
  CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
} from "@/config/claudeDesktopProviderPresets";
import {
  opencodeProviderPresets,
  opencodeNpmPackages,
  OPENCODE_PRESET_MODEL_VARIANTS,
} from "@/config/opencodeProviderPresets";
import {
  openclawProviderPresets,
  openclawApiProtocols,
} from "@/config/openclawProviderPresets";
import {
  hermesProviderPresets,
  hermesApiModes,
  HERMES_DEFAULT_API_MODE,
  HERMES_PROVIDER_SOURCE_FIELD,
  HERMES_PROVIDER_SOURCE_CUSTOM_LIST,
  HERMES_PROVIDER_SOURCE_DICT,
} from "@/config/hermesProviderPresets";
import { universalProviderPresets } from "@/config/universalProviderPresets";
import { CODING_PLAN_PROVIDERS } from "@/config/codingPlanProviders";
import { mcpPresets } from "@/config/mcpPresets";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import { USER_AGENT_PRESETS } from "@/config/userAgentPresets";

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "../crates/cc-switch-presets/data");
fs.mkdirSync(outDir, { recursive: true });

const write = (name: string, value: unknown) => {
  const file = path.join(outDir, `${name}.json`);
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n");
  const count = Array.isArray(value) ? value.length : Object.keys(value as object).length;
  console.log(`${name}.json: ${count} entries`);
};

write("claude", providerPresets);
write("codex", codexProviderPresets);
write("gemini", geminiProviderPresets);
write("claude_desktop", claudeDesktopProviderPresets);
write("opencode", opencodeProviderPresets);
write("openclaw", openclawProviderPresets);
write("hermes", hermesProviderPresets);
write("universal", universalProviderPresets);

write("meta", {
  claudeDesktopRoleRouteIds: CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
  opencodeNpmPackages,
  opencodePresetModelVariants: OPENCODE_PRESET_MODEL_VARIANTS,
  openclawApiProtocols,
  hermesApiModes,
  hermesDefaultApiMode: HERMES_DEFAULT_API_MODE,
  hermesProviderSource: {
    field: HERMES_PROVIDER_SOURCE_FIELD,
    customList: HERMES_PROVIDER_SOURCE_CUSTOM_LIST,
    dict: HERMES_PROVIDER_SOURCE_DICT,
  },
  userAgentPresets: USER_AGENT_PRESETS,
  codexCustomTemplate: getCodexCustomTemplate(),
  // Reference outputs of the Codex template builders; the Rust port is
  // tested against these.
  codexThirdPartyExample: {
    auth: generateThirdPartyAuth("sk-test"),
    config: generateThirdPartyConfig("Example Provider", "https://api.example.com/v1"),
    configWithModel: generateThirdPartyConfig("Example Provider", "https://api.example.com/v1", "gpt-5.5-mini"),
  },
  // RegExp is not JSON; keep source + flags.
  codingPlanProviders: CODING_PLAN_PROVIDERS.map((p) => ({
    id: p.id,
    label: p.label,
    pattern: p.pattern.source,
    flags: p.pattern.flags,
  })),
  // Dumped on a non-Windows host: `npx <pkg>`; the backend wraps with
  // `cmd /c` on Windows exactly like createNpxCommand() did.
  mcpPresets,
});

// vite-node keeps its server alive otherwise.
process.exit(0);
