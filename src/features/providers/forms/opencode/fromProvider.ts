import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import { OPENCODE_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

export type OmoCategory = "omo" | "omo-slim";

/** OMO / OMO Slim 供应商：编辑的是 agents/categories 草稿，而不是 provider 配置 */
export const isOmoCategory = (
  category: ProviderCategory | undefined,
): category is OmoCategory => category === "omo" || category === "omo-slim";

export interface OpenCodeFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
  /** 已保存的 OMO 配置，作为 OMO 草稿的初始值；不是 OMO 供应商时为 undefined */
  initialOmoSettings?: Record<string, unknown>;
}

/**
 * 由已保存的供应商（或新建时的空值）得到 OpenCode 编辑器的初始状态。
 * 端点、计费、托管账号等不在界面上编辑，保存时原样沿用。
 */
export const opencodeFormFromProvider = (
  initialData?: ProviderFormInitialData,
): OpenCodeFormInitialState => ({
  ...coreFormFromProvider(initialData),
  defaultValues: formDefaultValues(initialData, OPENCODE_DEFAULT_CONFIG),
  initialOmoSettings: isOmoCategory(initialData?.category)
    ? initialData?.settingsConfig
    : undefined,
});
