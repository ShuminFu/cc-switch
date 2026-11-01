import { Monitor, Moon, Sun } from "lucide-react";
import { SegmentedControl } from "@/components/common/SegmentedControl";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/components/theme-provider";

export function ThemeSettings() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.theme")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.themeHint")}
        </p>
      </header>
      <SegmentedControl
        ariaLabel={t("settings.theme")}
        value={theme}
        onChange={(next) => setTheme(next)}
        options={[
          { value: "light", label: t("settings.themeLight"), icon: Sun },
          { value: "dark", label: t("settings.themeDark"), icon: Moon },
          { value: "system", label: t("settings.themeSystem"), icon: Monitor },
        ]}
      />
    </section>
  );
}
