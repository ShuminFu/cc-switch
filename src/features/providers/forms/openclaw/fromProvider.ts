import type { ProviderFormData } from "@/lib/schemas/provider";
import { OPENCLAW_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

export interface OpenClawFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
}

/**
 * 由已保存的供应商（或新建时的空值）得到 OpenClaw 编辑器的初始状态。
 * 端点、计费、托管账号等不在界面上编辑，保存时原样沿用。
 */
export const openclawFormFromProvider = (
  initialData?: ProviderFormInitialData,
): OpenClawFormInitialState => ({
  ...coreFormFromProvider(initialData),
  defaultValues: formDefaultValues(initialData, OPENCLAW_DEFAULT_CONFIG),
});
