import { vi } from "vitest";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for gemini.
const entries = geminiProviderPresets.map((preset, index) => ({
  id: `gemini-${index}`,
  preset,
}));

describeSettingsConfigGoldens("gemini", entries);
