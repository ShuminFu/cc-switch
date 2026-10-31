import { useState } from "react";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { DirectorySettings } from "@/components/settings/DirectorySettings";
import { useDirectorySettings } from "@/hooks/useDirectorySettings";
import { directoryValuesByApp } from "@/hooks/settingsDirectoryFields";
import type { SettingsFormState } from "@/hooks/useSettingsForm";

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getAppConfigDirOverride: vi.fn(async () => null),
    getConfigDir: vi.fn(async () => ""),
    selectConfigDirectory: vi.fn(),
    setAppConfigDirOverride: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(async () => "/home/mock"),
  join: vi.fn(async (...segments: string[]) => segments.join("/")),
}));

let latestSettings: SettingsFormState | null = null;

function Harness() {
  const [settings, setSettings] = useState<SettingsFormState>({
    language: "en",
  } as SettingsFormState);
  latestSettings = settings;
  const dirs = useDirectorySettings({
    settings,
    onUpdateSettings: (updates) =>
      setSettings((prev) => ({ ...prev, ...updates })),
  });
  return (
    <DirectorySettings
      appConfigDir={dirs.appConfigDir}
      resolvedDirs={dirs.resolvedDirs}
      onAppConfigChange={dirs.updateAppConfigDir}
      onBrowseAppConfig={dirs.browseAppConfigDir}
      onResetAppConfig={dirs.resetAppConfigDir}
      directories={directoryValuesByApp(settings)}
      onDirectoryChange={dirs.updateDirectory}
      onBrowseDirectory={dirs.browseDirectory}
      onResetDirectory={dirs.resetDirectory}
    />
  );
}

describe("DirectorySettings inputs", () => {
  beforeEach(() => {
    latestSettings = null;
  });

  it("keeps spaces while typing and trims only on blur", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const input = screen.getByLabelText(
      "Claude Code Configuration Directory",
    ) as HTMLInputElement;
    await waitFor(() => expect(input.placeholder).toBe("/home/mock/.claude"));

    await user.type(input, "a b ");
    expect(input.value).toBe("a b ");
    expect(latestSettings?.claudeConfigDir).toBe("a b ");

    fireEvent.blur(input);
    expect(input.value).toBe("a b");
    expect(latestSettings?.claudeConfigDir).toBe("a b");
  });

  it("clearing the field removes the override and shows the default as placeholder", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const input = screen.getByLabelText(
      "Claude Code Configuration Directory",
    ) as HTMLInputElement;
    await waitFor(() => expect(input.placeholder).toBe("/home/mock/.claude"));

    await user.type(input, "/custom");
    expect(latestSettings?.claudeConfigDir).toBe("/custom");

    await user.clear(input);
    expect(input.value).toBe("");
    expect(input.placeholder).toBe("/home/mock/.claude");
    expect(latestSettings?.claudeConfigDir).toBeUndefined();
  });
});
