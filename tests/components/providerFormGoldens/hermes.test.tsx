import { vi } from "vitest";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for hermes.
const entries = hermesProviderPresets.map((preset, index) => ({
  id: `hermes-${index}`,
  preset,
}));

describeSettingsConfigGoldens("hermes", entries);
