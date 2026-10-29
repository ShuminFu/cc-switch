import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { OmoFormFields } from "@/components/providers/forms/OmoFormFields";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import type { OmoDraftState } from "@/components/providers/forms/hooks/useOmoDraftState";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { ProviderCategory } from "@/types";
import { SettingsConfigJsonField } from "../SettingsConfigJsonField";
import { isOmoCategory } from "./fromProvider";

type OpenCodeFormFieldsProps = ComponentProps<typeof OpenCodeFormFields>;
type OmoFormFieldsProps = ComponentProps<typeof OmoFormFields>;

const OPENCODE_CONFIG_PLACEHOLDER = `{
  "npm": "@ai-sdk/openai-compatible",
  "options": {
    "baseURL": "https://your-api-endpoint.com",
    "apiKey": "your-api-key-here"
  },
  "models": {}
}`;

export interface OpenCodeFieldsProps {
  category?: ProviderCategory;
  fields: Omit<OpenCodeFormFieldsProps, "category">;
  omoDraft: OmoDraftState;
  omoModelSource: Pick<
    OmoFormFieldsProps,
    "modelOptions" | "modelVariantsMap" | "presetMetaMap"
  >;
}

/**
 * OpenCode 的专属字段。OMO / OMO Slim 编辑 agents/categories 草稿，下方是
 * 只读的合并预览；普通供应商编辑 provider 配置与 settingsConfig JSON。
 */
export function OpenCodeFields({
  category,
  fields,
  omoDraft,
  omoModelSource,
}: OpenCodeFieldsProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();

  if (!isOmoCategory(category)) {
    return (
      <>
        <OpenCodeFormFields {...fields} category={category} />
        <SettingsConfigJsonField placeholder={OPENCODE_CONFIG_PLACEHOLDER} />
      </>
    );
  }

  const isFullOmo = category === "omo";
  return (
    <>
      <OmoFormFields
        {...omoModelSource}
        agents={omoDraft.omoAgents}
        onAgentsChange={omoDraft.setOmoAgents}
        categories={isFullOmo ? omoDraft.omoCategories : undefined}
        onCategoriesChange={isFullOmo ? omoDraft.setOmoCategories : undefined}
        otherFieldsStr={omoDraft.omoOtherFieldsStr}
        onOtherFieldsStrChange={omoDraft.setOmoOtherFieldsStr}
        isSlim={!isFullOmo}
      />
      <div className="space-y-2">
        <Label>{t("provider.configJson")}</Label>
        <JsonEditor
          value={omoDraft.mergedOmoJsonPreview}
          onChange={() => {}}
          rows={14}
          showValidation={false}
          language="json"
          darkMode={isDarkMode}
        />
      </div>
    </>
  );
}
