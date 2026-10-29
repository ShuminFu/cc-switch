import type { AppId } from "@/lib/api";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory, ProviderMeta } from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";

export type ProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  meta?: ProviderMeta;
  providerKey?: string; // OpenCode/OpenClaw: user-defined provider key
  suggestedDefaults?: OpenClawSuggestedDefaults; // OpenClaw: suggested default model configuration
};

export interface ProviderFormInitialData {
  name?: string;
  websiteUrl?: string;
  notes?: string;
  settingsConfig?: Record<string, unknown>;
  category?: ProviderCategory;
  meta?: ProviderMeta;
  icon?: string;
  iconColor?: string;
}

export interface ProviderFormProps {
  appId: AppId;
  providerId?: string;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  onSubmittingChange?: (isSubmitting: boolean) => void;
  /** 表单是否有未保存的修改（RHF dirty 或编辑器被编辑过），用于关闭前确认 */
  onDirtyChange?: (isDirty: boolean) => void;
  initialData?: ProviderFormInitialData;
  showButtons?: boolean;
  isProxyTakeover?: boolean;
}
