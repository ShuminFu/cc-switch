import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ExternalChangeBannerProps {
  onReload: () => void;
  onKeepMine: () => void;
  className?: string;
}

/**
 * Shown when the underlying file changed while the user has unsaved edits
 * (see `useEditableResource().externalChange`).
 */
export function ExternalChangeBanner({
  onReload,
  onKeepMine,
  className,
}: ExternalChangeBannerProps) {
  const { t } = useTranslation();
  return (
    <div
      role="alert"
      className={cn(
        "flex flex-wrap items-center justify-between gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm",
        className,
      )}
    >
      <span className="flex items-center gap-2 text-amber-800 dark:text-amber-300">
        <RefreshCw className="h-4 w-4 shrink-0" />
        {t("common.externalChangeMessage")}
      </span>
      <div className="flex items-center gap-2">
        <Button type="button" size="sm" variant="outline" onClick={onReload}>
          {t("common.externalChangeReload")}
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={onKeepMine}>
          {t("common.externalChangeKeepMine")}
        </Button>
      </div>
    </div>
  );
}
