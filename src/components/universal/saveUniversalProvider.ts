import type { TFunction } from "i18next";
import { toast } from "sonner";
import { universalProvidersApi } from "@/lib/api";
import type { UniversalProvider } from "@/types";

export type UniversalSaveResult =
  /** Upsert failed: nothing was stored. */
  | "saveFailed"
  /** Stored (and synced when asked). */
  | "saved"
  /** Stored, but syncing it into the apps failed. */
  | "syncFailed";

/**
 * Save a universal provider and optionally sync it into the apps, with
 * distinct toasts for the two phases (FE-PROV-47). A sync failure after a
 * successful upsert is reported as such instead of as "save failed", so the
 * caller can refresh its list and close the form.
 */
export async function saveUniversalProvider(
  provider: UniversalProvider,
  { sync, isEdit, t }: { sync: boolean; isEdit: boolean; t: TFunction },
): Promise<UniversalSaveResult> {
  try {
    await universalProvidersApi.upsert(provider);
  } catch (error) {
    console.error("[UniversalProvider] Failed to save", error);
    toast.error(
      isEdit
        ? t("universalProvider.saveError", {
            defaultValue: "保存统一供应商失败",
          })
        : t("universalProvider.addFailed", {
            defaultValue: "统一供应商添加失败",
          }),
    );
    return "saveFailed";
  }

  if (!sync) {
    toast.success(
      t("universalProvider.updated", { defaultValue: "统一供应商已更新" }),
    );
    return "saved";
  }

  try {
    await universalProvidersApi.sync(provider.id);
  } catch (error) {
    console.error("[UniversalProvider] Saved but sync failed", error);
    toast.warning(
      isEdit
        ? t("universalProvider.savedButSyncFailed", {
            defaultValue: "统一供应商已保存，但同步失败",
          })
        : t("universalProvider.addedButSyncFailed", {
            defaultValue: "统一供应商已添加，但同步失败",
          }),
    );
    return "syncFailed";
  }

  toast.success(
    isEdit
      ? t("universalProvider.savedAndSynced", {
          defaultValue: "已保存并同步到所有应用",
        })
      : t("universalProvider.addedAndSynced", {
          defaultValue: "统一供应商已添加并同步",
        }),
  );
  return "saved";
}
