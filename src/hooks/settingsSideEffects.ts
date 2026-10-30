import type { TFunction } from "i18next";
import { toast } from "sonner";
import { providersApi, settingsApi } from "@/lib/api";
import type { Settings } from "@/types";
import { syncCurrentProvidersLiveSafe } from "@/utils/postChangeSync";
import { changedDirectoryFields } from "./settingsDirectoryFields";
import type { SettingsFormState } from "./settingsPayload";

/**
 * `auto`: the General tab's instant save of a few fields (`updates`).
 * `full`: the explicit Save button, which also commits directory overrides.
 */
export type SaveScope = "auto" | "full";

export interface SideEffectContext {
  scope: SaveScope;
  /** The payload that was just persisted. */
  next: Settings;
  /** Settings as persisted before this save (live query cache first). */
  prev: Settings | null | undefined;
  /** Fields the caller changed (autosave) or overrode (explicit save). */
  updates: Partial<SettingsFormState>;
  /** Sanitised CC Switch app config dir override (full scope only). */
  appConfigDir?: string;
  t: TFunction;
  /** Set once current providers were re-written to live config. */
  providersSynced: boolean;
}

export interface SettingsSideEffect {
  id: string;
  scopes: readonly SaveScope[];
  run: (ctx: SideEffectContext) => Promise<void>;
}

/**
 * Whether `field` changed in this save. An autosave only considers the fields
 * it was asked to change, so a stale value elsewhere in the form never fires
 * an unrelated system call.
 */
export function fieldChanged<K extends keyof Settings>(
  ctx: SideEffectContext,
  field: K,
  fallback?: Settings[K],
): boolean {
  if (ctx.scope === "auto" && !(field in ctx.updates)) return false;
  const nextValue = ctx.next[field] ?? fallback;
  const prevValue = ctx.prev?.[field] ?? fallback;
  return nextValue !== undefined && nextValue !== prevValue;
}

const BOTH: readonly SaveScope[] = ["auto", "full"];
const FULL_ONLY: readonly SaveScope[] = ["full"];

async function syncClaudePlugin(ctx: SideEffectContext): Promise<void> {
  const { t } = ctx;
  const enabled = ctx.next.enableClaudePluginIntegration;
  const failed = () =>
    toast.error(
      t("notifications.syncClaudePluginFailed", {
        defaultValue: "同步 Claude 插件失败",
      }),
    );
  try {
    let official = true;
    if (enabled) {
      const currentId = await providersApi.getCurrent("claude");
      official = false;
      if (currentId) {
        const allProviders = await providersApi.getAll("claude");
        official = allProviders[currentId]?.category === "official";
      }
    }
    await settingsApi.applyClaudePluginConfig({ official });

    const syncResult = await syncCurrentProvidersLiveSafe();
    if (!syncResult.ok) {
      console.warn(
        "[useSettings] Failed to sync providers after toggling Claude plugin",
        syncResult.error,
      );
      failed();
    }
    ctx.providersSynced = true;
  } catch (error) {
    console.warn("[useSettings] Failed to sync Claude plugin config", error);
    failed();
  }
}

/**
 * One entry per setting with a side effect beyond `save_settings`, run in
 * order after the settings were persisted. Effects report their own
 * non-fatal failures; an effect that throws aborts the save.
 */
export const SETTINGS_SIDE_EFFECTS: readonly SettingsSideEffect[] = [
  {
    id: "appConfigDir",
    scopes: FULL_ONLY,
    run: async (ctx) => {
      await settingsApi.setAppConfigDirOverride(ctx.appConfigDir ?? null);
    },
  },
  {
    id: "launchOnStartup",
    scopes: BOTH,
    run: async (ctx) => {
      if (!fieldChanged(ctx, "launchOnStartup")) return;
      try {
        await settingsApi.setAutoLaunch(Boolean(ctx.next.launchOnStartup));
      } catch (error) {
        console.error("Failed to update auto-launch:", error);
        toast.error(
          ctx.t("settings.autoLaunchFailed", {
            defaultValue: "设置开机自启失败",
          }),
        );
      }
    },
  },
  {
    // Claude Code 初次安装确认：开=写入 hasCompletedOnboarding=true；关=删除该字段
    id: "skipClaudeOnboarding",
    scopes: BOTH,
    run: async (ctx) => {
      if (!fieldChanged(ctx, "skipClaudeOnboarding", false)) return;
      const skip = ctx.next.skipClaudeOnboarding ?? false;
      try {
        if (skip) {
          await settingsApi.applyClaudeOnboardingSkip();
        } else {
          await settingsApi.clearClaudeOnboardingSkip();
        }
      } catch (error) {
        console.warn(
          "[useSettings] Failed to sync Claude onboarding skip",
          error,
        );
        toast.error(
          skip
            ? ctx.t("notifications.skipClaudeOnboardingFailed", {
                defaultValue: "跳过 Claude Code 初次安装确认失败",
              })
            : ctx.t("notifications.clearClaudeOnboardingSkipFailed", {
                defaultValue: "恢复 Claude Code 初次安装确认失败",
              }),
        );
      }
    },
  },
  {
    // 同步 Claude 插件集成配置到 ~/.claude/settings.json
    id: "enableClaudePluginIntegration",
    scopes: BOTH,
    run: async (ctx) => {
      if (!fieldChanged(ctx, "enableClaudePluginIntegration")) return;
      await syncClaudePlugin(ctx);
    },
  },
  {
    id: "language",
    scopes: BOTH,
    run: async (ctx) => {
      if (ctx.scope === "auto" && !("language" in ctx.updates)) return;
      try {
        if (typeof window !== "undefined" && ctx.next.language) {
          window.localStorage.setItem("language", ctx.next.language);
        }
      } catch (error) {
        console.warn(
          "[useSettings] Failed to persist language preference",
          error,
        );
      }
    },
  },
  {
    id: "trayMenu",
    scopes: BOTH,
    run: async () => {
      try {
        await providersApi.updateTrayMenu();
      } catch (error) {
        console.warn("[useSettings] Failed to refresh tray menu", error);
      }
    },
  },
  {
    // 目录覆盖变化后把"当前供应商"写回对应应用的 live 配置（插件同步已做过则跳过）
    id: "configDirectories",
    scopes: FULL_ONLY,
    run: async (ctx) => {
      if (ctx.providersSynced) return;
      if (changedDirectoryFields(ctx.next, ctx.prev).length === 0) return;
      const syncResult = await syncCurrentProvidersLiveSafe();
      if (!syncResult.ok) {
        console.warn(
          "[useSettings] Failed to sync current providers after directory change",
          syncResult.error,
        );
      }
      ctx.providersSynced = true;
    },
  },
];

export async function runSettingsSideEffects(
  ctx: SideEffectContext,
  effects: readonly SettingsSideEffect[] = SETTINGS_SIDE_EFFECTS,
): Promise<void> {
  for (const effect of effects) {
    if (effect.scopes.includes(ctx.scope)) {
      await effect.run(ctx);
    }
  }
}
