import { vi } from "vitest";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for opencode.
const entries = opencodeProviderPresets.map((preset, index) => ({
  id: `opencode-${index}`,
  preset,
}));

describeSettingsConfigGoldens("opencode", entries);
