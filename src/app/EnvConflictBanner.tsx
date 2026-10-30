import { useEffect, useState } from "react";
import type { AppId } from "@/lib/api";
import type { EnvConflict } from "@/types/env";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";

const DISMISSED_KEY = "env_banner_dismissed";

const isDismissed = () => {
  try {
    return Boolean(sessionStorage.getItem(DISMISSED_KEY));
  } catch {
    return false;
  }
};

const conflictKey = (c: EnvConflict) => `${c.varName}:${c.sourcePath}`;

/**
 * Environment-variable conflict banner: checks all apps at startup and the
 * active app whenever it changes.
 */
export function EnvConflictBanner({ activeApp }: { activeApp: AppId }) {
  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  useEffect(() => {
    const checkEnvOnStartup = async () => {
      try {
        const allConflicts = await checkAllEnvConflicts();
        const flatConflicts = Object.values(allConflicts).flat();

        if (flatConflicts.length > 0) {
          setEnvConflicts(flatConflicts);
          if (!isDismissed()) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on startup:",
          error,
        );
      }
    };

    void checkEnvOnStartup();
  }, []);

  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      try {
        const conflicts = await checkEnvConflicts(activeApp);

        if (conflicts.length > 0) {
          setEnvConflicts((prev) => {
            const existingKeys = new Set(prev.map(conflictKey));
            return [
              ...prev,
              ...conflicts.filter((c) => !existingKeys.has(conflictKey(c))),
            ];
          });
          if (!isDismissed()) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on app switch:",
          error,
        );
      }
    };

    void checkEnvOnSwitch();
  }, [activeApp]);

  if (!showEnvBanner || envConflicts.length === 0) {
    return null;
  }

  return (
    <EnvWarningBanner
      conflicts={envConflicts}
      onDismiss={() => {
        setShowEnvBanner(false);
        try {
          sessionStorage.setItem(DISMISSED_KEY, "true");
        } catch {
          // ignore unavailable storage
        }
      }}
      onDeleted={async () => {
        try {
          const allConflicts = await checkAllEnvConflicts();
          const flatConflicts = Object.values(allConflicts).flat();
          setEnvConflicts(flatConflicts);
          if (flatConflicts.length === 0) {
            setShowEnvBanner(false);
          }
        } catch (error) {
          console.error(
            "[App] Failed to re-check conflicts after deletion:",
            error,
          );
        }
      }}
    />
  );
}
