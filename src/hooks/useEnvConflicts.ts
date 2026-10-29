import { useEffect, useState } from "react";
import type { AppId } from "@/lib/api";
import type { EnvConflict } from "@/types/env";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";

const BANNER_DISMISSED_KEY = "env_banner_dismissed";

const conflictKey = (c: EnvConflict) => `${c.varName}:${c.sourcePath}`;

const isBannerDismissed = () =>
  Boolean(sessionStorage.getItem(BANNER_DISMISSED_KEY));

/**
 * 环境变量冲突检测：启动时检查全部应用，切换应用时增量检查当前应用。
 * 横幅在本会话内被关闭后不再自动弹出。
 */
export function useEnvConflicts(activeApp: AppId) {
  const [conflicts, setConflicts] = useState<EnvConflict[]>([]);
  const [showBanner, setShowBanner] = useState(false);

  useEffect(() => {
    const checkEnvOnStartup = async () => {
      try {
        const allConflicts = await checkAllEnvConflicts();
        const flatConflicts = Object.values(allConflicts).flat();

        if (flatConflicts.length > 0) {
          setConflicts(flatConflicts);
          if (!isBannerDismissed()) {
            setShowBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on startup:",
          error,
        );
      }
    };

    checkEnvOnStartup();
  }, []);

  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      try {
        const appConflicts = await checkEnvConflicts(activeApp);

        if (appConflicts.length > 0) {
          setConflicts((prev) => {
            const existingKeys = new Set(prev.map(conflictKey));
            const newConflicts = appConflicts.filter(
              (c) => !existingKeys.has(conflictKey(c)),
            );
            return [...prev, ...newConflicts];
          });
          if (!isBannerDismissed()) {
            setShowBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on app switch:",
          error,
        );
      }
    };

    checkEnvOnSwitch();
  }, [activeApp]);

  const dismiss = () => {
    setShowBanner(false);
    sessionStorage.setItem(BANNER_DISMISSED_KEY, "true");
  };

  /** 删除冲突变量后重新检查，全部清除时隐藏横幅 */
  const recheck = async () => {
    try {
      const allConflicts = await checkAllEnvConflicts();
      const flatConflicts = Object.values(allConflicts).flat();
      setConflicts(flatConflicts);
      if (flatConflicts.length === 0) {
        setShowBanner(false);
      }
    } catch (error) {
      console.error(
        "[App] Failed to re-check conflicts after deletion:",
        error,
      );
    }
  };

  return {
    conflicts,
    showBanner: showBanner && conflicts.length > 0,
    dismiss,
    recheck,
  };
}
