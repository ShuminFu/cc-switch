import type { TFunction } from "i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory, ProviderMeta } from "@/types";
import { parseOmoOtherFieldsObject } from "@/types/omo";
import type { OmoDraftState } from "@/components/providers/forms/hooks/useOmoDraftState";
import { additiveProviderMeta } from "../additiveForm";
import type { CoreFormInitialState } from "../coreForm";
import type { ProviderFormInitialData } from "../types";
import { isOmoCategory } from "./fromProvider";

type OmoDraft = Pick<
  OmoDraftState,
  "omoAgents" | "omoCategories" | "omoOtherFieldsStr"
>;

/**
 * OMO「其他字段」JSON 的阻断错误（格式错了保存下去数据就坏了）。
 * 为空时不校验。
 */
export const omoOtherFieldsError = (
  otherFieldsStr: string,
  t: TFunction,
): string | null => {
  if (!otherFieldsStr.trim()) return null;
  try {
    return parseOmoOtherFieldsObject(otherFieldsStr)
      ? null
      : t("omo.jsonMustBeObject", { field: t("omo.otherFields") });
  } catch {
    return t("omo.invalidJson");
  }
};

/**
 * OpenCode 的 settingsConfig。OMO / OMO Slim 由草稿拼出
 * `{agents, categories?, otherFields?}`（OMO Slim 没有 categories，空的部分省略）；
 * 普通供应商就是 JSON 编辑器里的文本。
 */
export const toOpenCodeSettingsConfig = ({
  values,
  category,
  omoDraft,
}: {
  values: ProviderFormData;
  category: ProviderCategory | undefined;
  omoDraft: OmoDraft;
}): string => {
  if (!isOmoCategory(category)) {
    return values.settingsConfig.trim();
  }

  const omoConfig: Record<string, unknown> = {};
  if (Object.keys(omoDraft.omoAgents).length > 0) {
    omoConfig.agents = omoDraft.omoAgents;
  }
  if (category === "omo" && Object.keys(omoDraft.omoCategories).length > 0) {
    omoConfig.categories = omoDraft.omoCategories;
  }
  if (omoDraft.omoOtherFieldsStr.trim()) {
    // 格式已在提交前校验过（omoOtherFieldsError），此处可以安全解析
    const otherFields = parseOmoOtherFieldsObject(omoDraft.omoOtherFieldsStr);
    if (otherFields) {
      omoConfig.otherFields = otherFields;
    }
  }
  return JSON.stringify(omoConfig);
};

/**
 * 提交的 providerKey。普通供应商用用户填写的 key；OMO 新建时生成
 * `omo-xxxxxxxx` / `omo-slim-xxxxxxxx`，编辑时不带（沿用已有 key）。
 */
export const opencodeProviderKeyForSave = ({
  category,
  isEditMode,
  providerKey,
  randomId = () => crypto.randomUUID(),
}: {
  category: ProviderCategory | undefined;
  isEditMode: boolean;
  providerKey: string;
  randomId?: () => string;
}): string | undefined => {
  if (!isOmoCategory(category)) return providerKey;
  if (isEditMode) return undefined;
  return `${category}-${randomId().slice(0, 8)}`;
};

/** 保存时的 meta：沿用已有 meta，应用相关字段一律清掉 */
export const toOpenCodeProviderMeta = (
  initialData: ProviderFormInitialData | undefined,
  initial: CoreFormInitialState,
): ProviderMeta => additiveProviderMeta(initialData, initial);
