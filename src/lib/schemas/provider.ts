import { z } from "zod";
import i18next, { type TFunction } from "i18next";

/**
 * 解析 JSON 语法错误，提取位置信息（文案走 i18n）
 */
export function parseJsonError(error: unknown, t: TFunction): string {
  if (!(error instanceof SyntaxError)) {
    return t("providerForm.validation.configJsonInvalid");
  }

  const message = error.message;

  // 提取位置信息：Chrome/V8: "Unexpected token ... in JSON at position 123"
  const positionMatch = message.match(/at position (\d+)/i);
  if (positionMatch) {
    const position = parseInt(positionMatch[1], 10);
    return t("providerForm.validation.jsonSyntaxAtPosition", {
      detail: message.split(" in JSON")[0],
      position,
    });
  }

  // Firefox: "JSON.parse: unexpected character at line 1 column 23"
  const lineColumnMatch = message.match(/line (\d+) column (\d+)/i);
  if (lineColumnMatch) {
    return t("providerForm.validation.jsonSyntaxAtLineColumn", {
      line: lineColumnMatch[1],
      column: lineColumnMatch[2],
    });
  }

  // 通用情况：保留引擎给出的原始错误信息
  return t("providerForm.validation.jsonSyntax", {
    detail: message.replace(/^JSON\.parse:\s*/i, ""),
  });
}

/**
 * 供应商表单 schema。校验文案在校验时才通过 t 取，切换语言后无需重建 schema。
 */
export const createProviderSchema = (t: TFunction) =>
  z.object({
    name: z.string(), // 必填校验移至 handleSubmit 中用 toast 提示
    websiteUrl: z
      .string()
      .url({ error: () => t("providerForm.validation.invalidUrl") })
      .optional()
      .or(z.literal("")),
    notes: z.string().optional(),
    settingsConfig: z
      .string()
      .min(1, { error: () => t("providerForm.validation.configRequired") })
      .superRefine((value, ctx) => {
        try {
          JSON.parse(value);
        } catch (error) {
          ctx.addIssue({
            code: z.ZodIssueCode.custom,
            message: parseJsonError(error, t),
          });
        }
      }),
    // 图标配置
    icon: z.string().optional(),
    iconColor: z.string().optional(),
  });

/** 跟随全局 i18n 当前语言的默认实例（供判别联合等静态 schema 使用） */
export const providerSchema = createProviderSchema(
  i18next.t.bind(i18next) as TFunction,
);

export type ProviderFormData = z.infer<typeof providerSchema>;
