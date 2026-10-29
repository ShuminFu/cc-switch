import type { TFunction } from "i18next";
import type { TranslationKey } from "@/i18n/keys";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";

/**
 * 所有应用的供应商预设共享的字段。各应用的预设类型在此基础上扩展
 * 自己的配置字段（settingsConfig / auth+config / 路由等）。
 */
export interface BasePreset {
  name: string;
  nameKey?: TranslationKey; // i18n key for localized display name
  websiteUrl: string;
  // 第三方/聚合等可单独配置获取 API Key 的链接
  apiKeyUrl?: string;
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory;
  // 请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
}

/** 当前选中的预设中，提交时需要带上的信息 */
export interface ActivePreset {
  id: string;
  category?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
}

export const activePresetFrom = (
  id: string,
  preset: BasePreset,
): ActivePreset => ({
  id,
  category: preset.category,
  isPartner: preset.isPartner,
  partnerPromotionKey: preset.partnerPromotionKey,
});

export const presetDisplayName = (preset: BasePreset, t: TFunction): string =>
  preset.nameKey ? t(preset.nameKey) : preset.name;

/**
 * 选中预设后重置表单用的基础字段。备注不在其中：切换预设会清空备注，
 * 与拆分前的行为一致。
 */
export const presetBaseFormValues = (
  preset: BasePreset,
  settingsConfig: string,
  t: TFunction,
  name: string = presetDisplayName(preset, t),
): ProviderFormData => ({
  name,
  websiteUrl: preset.websiteUrl ?? "",
  settingsConfig,
  icon: preset.icon ?? "",
  iconColor: preset.iconColor ?? "",
});
