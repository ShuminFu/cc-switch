import { useState, useEffect } from "react";
import type { ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";
import type { BasePreset } from "@/features/providers/forms/basePreset";

interface UseProviderCategoryProps {
  appId: AppId;
  selectedPresetId: string | null;
  isEditMode: boolean;
  initialCategory?: ProviderCategory;
  /** 当前应用的完整预设表（按预设 id 中的序号查找）；未加载完时为 undefined */
  presets?: readonly BasePreset[];
  /** 预设未写 category 时，isOfficial 是否视为 official（Claude / Codex） */
  officialFallback?: boolean;
}

/**
 * 管理供应商类别状态
 * 根据选择的预设自动更新类别
 */
export function useProviderCategory({
  appId,
  selectedPresetId,
  isEditMode,
  initialCategory,
  presets,
  officialFallback = false,
}: UseProviderCategoryProps) {
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    // 编辑模式：使用 initialCategory
    isEditMode ? initialCategory : undefined,
  );

  useEffect(() => {
    // 编辑模式：只在初始化时设置，后续不自动更新
    if (isEditMode) {
      setCategory(initialCategory);
      return;
    }

    if (selectedPresetId === "custom") {
      setCategory("custom");
      return;
    }

    if (!selectedPresetId) return;

    // 从预设 ID 提取索引
    const match = selectedPresetId.match(
      /^(claude|codex|gemini|opencode|openclaw|hermes)-(\d+)$/,
    );
    if (!match) return;

    const [, type, indexStr] = match;
    // 预设前缀必须与当前应用一致
    if (type !== appId) return;
    const index = parseInt(indexStr, 10);

    const preset = presets?.[index];
    if (preset) {
      setCategory(
        preset.category ||
          (officialFallback && preset.isOfficial ? "official" : undefined),
      );
    }
  }, [
    appId,
    selectedPresetId,
    isEditMode,
    initialCategory,
    presets,
    officialFallback,
  ]);

  return { category, setCategory };
}
