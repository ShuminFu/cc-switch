import { useState, useEffect } from "react";
import type { ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";
import {
  CUSTOM_PRESET_ID,
  findPresetEntry,
  getPresetCategory,
} from "@/config/presetRegistry";

interface UseProviderCategoryProps {
  appId: AppId;
  selectedPresetId: string | null;
  isEditMode: boolean;
  initialCategory?: ProviderCategory;
}

/**
 * Provider category state: follows the selected preset in add mode, fixed to
 * the stored category in edit mode. Presets are resolved through the preset
 * registry by stable id (FE-PROV-19).
 */
export function useProviderCategory({
  appId,
  selectedPresetId,
  isEditMode,
  initialCategory,
}: UseProviderCategoryProps) {
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    isEditMode ? initialCategory : undefined,
  );

  useEffect(() => {
    if (isEditMode) {
      setCategory(initialCategory);
      return;
    }

    if (selectedPresetId === CUSTOM_PRESET_ID) {
      setCategory("custom");
      return;
    }

    const entry = findPresetEntry(appId, selectedPresetId);
    if (entry) {
      setCategory(getPresetCategory(entry.preset));
    }
  }, [appId, selectedPresetId, isEditMode, initialCategory]);

  return { category, setCategory };
}
