import { vi } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids and filter as GrokBuildProviderForm's grokPresetEntries.
const entries = codexProviderPresets
  .map((preset, index) => ({ id: `grokbuild-${index}`, preset }))
  .filter(({ preset }) => preset.category !== "official" && !preset.isOfficial);

describeSettingsConfigGoldens("grokbuild", entries);
