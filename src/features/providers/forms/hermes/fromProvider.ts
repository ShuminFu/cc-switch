import type { ProviderFormData } from "@/lib/schemas/provider";
import { HERMES_DEFAULT_CONFIG } from "@/components/providers/forms/hooks/useHermesFormState";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

export interface HermesFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
}

/**
 * 由已保存的供应商（或新建时的空值）得到 Hermes 编辑器的初始状态。
 * 端点、计费、托管账号等不在界面上编辑，保存时原样沿用。
 */
export const hermesFormFromProvider = (
  initialData?: ProviderFormInitialData,
): HermesFormInitialState => ({
  ...coreFormFromProvider(initialData),
  defaultValues: formDefaultValues(initialData, HERMES_DEFAULT_CONFIG),
});
