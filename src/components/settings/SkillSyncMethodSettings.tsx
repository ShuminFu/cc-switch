import { useTranslation } from "react-i18next";
import { SegmentedControl } from "@/components/common/SegmentedControl";
import type { SkillSyncMethod } from "@/types";

export interface SkillSyncMethodSettingsProps {
  value: SkillSyncMethod;
  onChange: (value: SkillSyncMethod) => void;
}

export function SkillSyncMethodSettings({
  value,
  onChange,
}: SkillSyncMethodSettingsProps) {
  const { t } = useTranslation();

  // Handle default values: undefined or "auto" defaults to symlink display
  const displayValue = value === "copy" ? "copy" : "symlink";

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.skillSync.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillSync.description")}
        </p>
      </header>
      <SegmentedControl
        ariaLabel={t("settings.skillSync.title")}
        value={displayValue}
        onChange={(next) => onChange(next)}
        options={[
          { value: "symlink", label: t("settings.skillSync.symlink") },
          { value: "copy", label: t("settings.skillSync.copy") },
        ]}
      />
      {displayValue === "symlink" && (
        <p className="text-xs text-muted-foreground">
          {t("settings.skillSync.symlinkHint")}
        </p>
      )}
    </section>
  );
}
