import { vi } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { describeSettingsConfigGoldens } from "./harness";

vi.mock("sonner", async () => (await import("./mocks")).sonnerMock);
vi.mock(
  "@/components/JsonEditor",
  async () => (await import("./mocks")).jsonEditorMock,
);

// Same ids as ClaudeDesktopProviderForm's presetEntries.
const entries = claudeDesktopProviderPresets.map((preset, index) => ({
  id: `claude-desktop-${index}`,
  preset,
}));

describeSettingsConfigGoldens("claude-desktop", entries);
