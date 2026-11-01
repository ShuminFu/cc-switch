import type { ComponentType, MutableRefObject, ReactNode } from "react";
import type { UseFormReturn } from "react-hook-form";
import type { AppId } from "@/lib/api";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory, ProviderMeta } from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { PresetEntry } from "@/config/presetRegistry";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";

/**
 * Provider form architecture (FE-PROV-17).
 *
 * `ProviderForm` is a thin shell (preset selector, basic fields, the
 * soft-validation confirm pipeline, pricing, buttons) that never branches on
 * the app. Everything app-specific lives in one *form adapter* per app,
 * registered in `adapters/index.ts` and selected through the app registry
 * (`APP_REGISTRY[appId].providerForm`). An adapter's `Body` is rendered as a
 * child component, so only the active app's hooks are mounted; it registers
 * its `validate` / `serialize` / `applyPreset` handlers with the shell through
 * `ctx.handleRef`.
 */

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
  initialData?: ProviderFormInitialData;
  showButtons?: boolean;
  isProxyTakeover?: boolean;
}

export type ProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  meta?: ProviderMeta;
  /** OpenCode/OpenClaw/Hermes: user-defined provider key. */
  providerKey?: string;
  /** OpenClaw: suggested default model configuration. */
  suggestedDefaults?: OpenClawSuggestedDefaults;
  /**
   * Edit mode: the custom speed-test endpoints the user applied in the
   * endpoint panel. Written by the edit dialog after the provider is saved,
   * so cancelling the dialog leaves them untouched (FE-PROV-10). Absent when
   * the endpoints were not changed.
   */
  customEndpoints?: string[];
};

/** Everything the shell hands to the active adapter's `Body`. */
export interface ProviderFormAdapterContext {
  appId: AppId;
  providerId?: string;
  initialData?: ProviderFormInitialData;
  isEditMode: boolean;
  isProxyTakeover: boolean;
  form: UseFormReturn<ProviderFormData>;
  /** Live (watched) `settingsConfig` text of the RHF form. */
  settingsConfig: string;
  setSettingsConfig: (config: string) => void;
  /** Read the current value outside render (event handlers). */
  getSettingsConfig: () => string;
  category?: ProviderCategory;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  isDarkMode: boolean;
  /** Basic fields (name, notes, website, icon) with an optional slot. */
  renderBasicFields: (beforeNameSlot?: ReactNode) => ReactNode;
  /** RHF error slot of the `settingsConfig` field. */
  settingsConfigErrorField: ReactNode;
  /** The adapter publishes its handlers here after every render. */
  handleRef: MutableRefObject<ProviderFormAdapterHandle | null>;
}

export interface AdapterValidation {
  /** Blocks saving; shown as an error toast. */
  error?: string | null;
  /** Business-rule problems the user may choose to save anyway. */
  issues?: string[];
}

export interface AdapterSerialization {
  /** Final settingsConfig JSON text. */
  settingsConfig: string;
  /**
   * App-specific meta fields. They are applied over the stored meta, so a
   * key set to `undefined` removes a stale value.
   */
  meta?: Partial<ProviderMeta>;
  providerKey?: string;
  /** Category to store when no preset supplied one (OMO). */
  presetCategory?: ProviderCategory;
  suggestedDefaults?: OpenClawSuggestedDefaults;
  /** Edit mode: applied custom endpoints (see `ProviderFormValues`). */
  customEndpoints?: string[];
}

export interface ProviderFormAdapterHandle {
  validate: (values: ProviderFormData) => AdapterValidation;
  /** `null` means serialisation failed and the adapter already told the user. */
  serialize: (values: ProviderFormData) => AdapterSerialization | null;
  /**
   * Apply a preset (`null` = back to "custom") to the adapter's own state and
   * optionally override the form values the shell resets to.
   */
  applyPreset: (
    entry: PresetEntry | null,
    formValues: ProviderFormData,
  ) => ProviderFormData;
}

export interface ShellFormAdapter {
  kind: "shell";
  Body: ComponentType<{ ctx: ProviderFormAdapterContext }>;
  /** settingsConfig JSON text of a new custom provider. */
  defaultSettingsConfig: string;
  /** Show the cost-multiplier / pricing section for this category. */
  showPricing: (category: ProviderCategory | undefined) => boolean;
  /** The app has a "common config" snippet (first-run notice applies). */
  hasCommonConfig: boolean;
}

/** Apps whose provider form is a complete component of its own. */
export interface StandaloneFormAdapter {
  kind: "standalone";
  Form: ComponentType<ProviderFormProps>;
}

export type ProviderFormAdapter = ShellFormAdapter | StandaloneFormAdapter;
