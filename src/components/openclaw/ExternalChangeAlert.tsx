import React from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

/**
 * Shown when the config changed on disk while the user has unsaved edits
 * (FE-UI-34): the edits are kept until the user chooses to reload.
 */
export const ExternalChangeAlert: React.FC<{ onReload: () => void }> = ({
  onReload,
}) => {
  const { t } = useTranslation();
  return (
    <Alert className="mb-4 border-amber-500/30 bg-amber-500/5" role="status">
      <AlertDescription className="flex items-center justify-between gap-3">
        <span>
          {t("openclaw.externalChange", {
            defaultValue:
              "This config changed on disk. Your unsaved edits are kept.",
          })}
        </span>
        <Button size="sm" variant="outline" onClick={onReload}>
          <RefreshCw className="w-3.5 h-3.5 mr-1" />
          {t("openclaw.reloadFromDisk", {
            defaultValue: "Discard edits and reload",
          })}
        </Button>
      </AlertDescription>
    </Alert>
  );
};
