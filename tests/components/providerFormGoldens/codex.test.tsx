import { vi } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ProviderForm's presetEntries for codex.
const entries = codexProviderPresets.map((preset, index) => ({
  id: `codex-${index}`,
  preset,
}));

describeSettingsConfigGoldens("codex", entries);
