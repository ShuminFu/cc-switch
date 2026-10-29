import { vi } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for claude.
const entries = providerPresets
  .filter((preset) => !preset.hidden)
  .map((preset, index) => ({ id: `claude-${index}`, preset }));

describeSettingsConfigGoldens("claude", entries, {
  knownRoundTripDrift: {
    // The preset ships both ANTHROPIC_AUTH_TOKEN and ANTHROPIC_API_KEY. Create
    // writes the key to AUTH_TOKEN without saving meta.apiKeyField; edit then
    // infers ANTHROPIC_API_KEY from the (empty) env entry and saves that.
    "claude-60": "apiKeyField inferred from an empty ANTHROPIC_API_KEY",
  },
});
