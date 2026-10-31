import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { skillsApi, type MigrationResult } from "@/lib/api/skills";
import type { SkillStorageLocation } from "@/types";
import { queryKeys } from "@/lib/query/keys";

/** How many per-skill errors the toast lists before summarising. */
const MAX_TOAST_ERRORS = 3;

export interface SkillStorageLocationSettingsProps {
  value: SkillStorageLocation;
  installedCount: number;
  onMigrated: (target: SkillStorageLocation) => void;
}

export function SkillStorageLocationSettings({
  value,
  installedCount,
  onMigrated,
}: SkillStorageLocationSettingsProps) {
  const { t } = useTranslation();
  const [pendingTarget, setPendingTarget] =
    useState<SkillStorageLocation | null>(null);
  const [isMigrating, setIsMigrating] = useState(false);
  // Per-skill failures of the last migration (FE-UI-58), as "<dir>: <reason>".
  const [migrationErrors, setMigrationErrors] = useState<string[]>([]);
  const queryClient = useQueryClient();

  const handleSelect = (target: SkillStorageLocation) => {
    if (target === value) return;
    if (installedCount > 0) {
      setPendingTarget(target);
    } else {
      doMigrate(target);
    }
  };

  const doMigrate = async (target: SkillStorageLocation) => {
    setIsMigrating(true);
    setPendingTarget(null);
    setMigrationErrors([]);
    try {
      const result: MigrationResult = await skillsApi.migrateStorage(target);
      // Installed skills moved (some of them at least): refetch their paths.
      await queryClient.invalidateQueries({ queryKey: queryKeys.skills.all });
      if (result.errors.length > 0) {
        setMigrationErrors(result.errors);
        const shown = result.errors.slice(0, MAX_TOAST_ERRORS).join("\n");
        const more = result.errors.length - MAX_TOAST_ERRORS;
        toast.warning(
          t("settings.skillStorage.migrationPartial", {
            migrated: result.migratedCount,
            errors: result.errors.length,
          }),
          {
            description:
              more > 0
                ? `${shown}\n${t("settings.skillStorage.migrationMoreErrors", {
                    defaultValue: "…and {{count}} more",
                    count: more,
                  })}`
                : shown,
            closeButton: true,
          },
        );
      } else {
        toast.success(
          t("settings.skillStorage.migrationSuccess", {
            count: result.migratedCount,
          }),
        );
      }
      onMigrated(target);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsMigrating(false);
    }
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.skillStorage.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillStorage.description")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <StorageButton
          active={value === "cc_switch"}
          disabled={isMigrating}
          onClick={() => handleSelect("cc_switch")}
        >
          {t("settings.skillStorage.ccSwitch")}
        </StorageButton>
        <StorageButton
          active={value === "unified"}
          disabled={isMigrating}
          onClick={() => handleSelect("unified")}
        >
          {isMigrating && value !== "unified" ? (
            <Loader2 size={14} className="mr-1 animate-spin" />
          ) : null}
          {t("settings.skillStorage.unified")}
        </StorageButton>
      </div>
      <p className="text-xs text-muted-foreground">
        {value === "unified"
          ? t("settings.skillStorage.unifiedHint")
          : t("settings.skillStorage.ccSwitchHint")}
      </p>

      {migrationErrors.length > 0 && (
        <div
          role="alert"
          data-testid="skill-migration-errors"
          className="space-y-1 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-800 dark:text-amber-300"
        >
          <p className="flex items-center gap-1 font-medium">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("settings.skillStorage.migrationErrorsTitle", {
              defaultValue:
                "These skills stayed in the previous location and need to be moved manually:",
            })}
          </p>
          <ul className="list-disc space-y-0.5 pl-5 font-mono">
            {migrationErrors.map((error) => (
              <li key={error} className="break-all">
                {error}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* 迁移确认对话框 */}
      <Dialog
        open={pendingTarget !== null}
        onOpenChange={(open) => {
          if (!open) setPendingTarget(null);
        }}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>{t("settings.skillStorage.confirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("settings.skillStorage.confirmMessage", {
                count: installedCount,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingTarget(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => pendingTarget && doMigrate(pendingTarget)}>
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}

interface StorageButtonProps {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function StorageButton({
  active,
  disabled,
  onClick,
  children,
}: StorageButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[96px]",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      {children}
    </Button>
  );
}
