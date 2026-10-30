import { describe, it, expect, beforeEach, vi } from "vitest";
import type { Settings } from "@/types";
import {
  APP_DIRECTORY_META,
  DIRECTORY_APP_IDS,
  DIRECTORY_SETTINGS_FIELDS,
  changedDirectoryFields,
  directoryValuesByApp,
} from "@/hooks/settingsDirectoryFields";
import {
  buildSettingsPayload,
  normalizeSettingsForm,
  type SettingsFormState,
} from "@/hooks/settingsPayload";
import {
  runSettingsSideEffects,
  type SideEffectContext,
} from "@/hooks/settingsSideEffects";

const api = vi.hoisted(() => ({
  setAppConfigDirOverride: vi.fn(),
  setAutoLaunch: vi.fn(),
  applyClaudeOnboardingSkip: vi.fn(),
  clearClaudeOnboardingSkip: vi.fn(),
  applyClaudePluginConfig: vi.fn(),
  syncCurrentProvidersLive: vi.fn(),
  updateTrayMenu: vi.fn(),
  getCurrent: vi.fn(),
  getAll: vi.fn(),
}));

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    setAppConfigDirOverride: api.setAppConfigDirOverride,
    setAutoLaunch: api.setAutoLaunch,
    applyClaudeOnboardingSkip: api.applyClaudeOnboardingSkip,
    clearClaudeOnboardingSkip: api.clearClaudeOnboardingSkip,
    applyClaudePluginConfig: api.applyClaudePluginConfig,
    syncCurrentProvidersLive: api.syncCurrentProvidersLive,
  },
  providersApi: {
    updateTrayMenu: api.updateTrayMenu,
    getCurrent: api.getCurrent,
    getAll: api.getAll,
  },
}));

const t = ((key: string) => key) as unknown as SideEffectContext["t"];

const baseForm = (): SettingsFormState =>
  ({
    showInTray: true,
    minimizeToTrayOnClose: true,
    language: "en",
  }) as SettingsFormState;

const ctx = (overrides: Partial<SideEffectContext>): SideEffectContext => ({
  scope: "full",
  next: baseForm(),
  prev: baseForm(),
  updates: {},
  t,
  providersSynced: false,
  ...overrides,
});

describe("settings save pipeline: directory fields", () => {
  it("covers every *ConfigDir field exactly once", () => {
    const fields = DIRECTORY_APP_IDS.map(
      (app) => APP_DIRECTORY_META[app].field,
    );
    expect([...fields].sort()).toEqual([...DIRECTORY_SETTINGS_FIELDS].sort());
    expect(new Set(fields).size).toBe(fields.length);
  });

  describe.each(DIRECTORY_APP_IDS)("%s", (app) => {
    const field = APP_DIRECTORY_META[app].field;

    it("is trimmed by form normalisation and payload building", () => {
      const form = normalizeSettingsForm(
        { ...baseForm(), [field]: "  /custom/dir  " } as Settings,
        () => "en",
      );
      expect(form[field]).toBe("/custom/dir");

      const payload = buildSettingsPayload(
        { ...baseForm(), [field]: "  /x  " },
        { [field]: "  /x  " },
      );
      expect(payload[field]).toBe("/x");
      expect(
        buildSettingsPayload(baseForm(), { [field]: "   " })[field],
      ).toBeUndefined();
      expect(directoryValuesByApp({ [field]: "/raw " })[app]).toBe("/raw ");
    });

    it("is detected as a change and triggers one live sync on explicit save", async () => {
      expect(
        changedDirectoryFields({ [field]: "/new" }, { [field]: " /old " }),
      ).toEqual([field]);
      expect(
        changedDirectoryFields({ [field]: " /same" }, { [field]: "/same " }),
      ).toEqual([]);

      api.syncCurrentProvidersLive.mockClear();
      await runSettingsSideEffects(
        ctx({ next: { ...baseForm(), [field]: "/new" } }),
      );
      expect(api.syncCurrentProvidersLive).toHaveBeenCalledTimes(1);
    });

    it("never triggers a live sync from an autosave", async () => {
      api.syncCurrentProvidersLive.mockClear();
      await runSettingsSideEffects(
        ctx({
          scope: "auto",
          next: { ...baseForm(), [field]: "/new" },
          updates: { [field]: "/new" },
        }),
      );
      expect(api.syncCurrentProvidersLive).not.toHaveBeenCalled();
    });
  });
});

describe("settings save pipeline: side-effect scopes", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
    api.syncCurrentProvidersLive.mockResolvedValue(undefined);
  });

  it("autosave only fires effects for the fields it was asked to change", async () => {
    await runSettingsSideEffects(
      ctx({
        scope: "auto",
        next: {
          ...baseForm(),
          launchOnStartup: true,
          skipClaudeOnboarding: true,
        },
        prev: { ...baseForm(), launchOnStartup: false },
        updates: { minimizeToTrayOnClose: false },
      }),
    );
    expect(api.setAutoLaunch).not.toHaveBeenCalled();
    expect(api.applyClaudeOnboardingSkip).not.toHaveBeenCalled();
    expect(api.setAppConfigDirOverride).not.toHaveBeenCalled();
    expect(api.updateTrayMenu).toHaveBeenCalledTimes(1);
  });

  it("explicit save fires every changed effect and the app dir override", async () => {
    await runSettingsSideEffects(
      ctx({
        next: {
          ...baseForm(),
          launchOnStartup: true,
          skipClaudeOnboarding: true,
        },
        prev: { ...baseForm(), launchOnStartup: false },
        appConfigDir: "/app",
      }),
    );
    expect(api.setAppConfigDirOverride).toHaveBeenCalledWith("/app");
    expect(api.setAutoLaunch).toHaveBeenCalledWith(true);
    expect(api.applyClaudeOnboardingSkip).toHaveBeenCalledTimes(1);
  });
});
