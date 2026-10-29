import { vi } from "vitest";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for openclaw.
const entries = openclawProviderPresets.map((preset, index) => ({
  id: `openclaw-${index}`,
  preset,
}));

describeSettingsConfigGoldens("openclaw", entries);
