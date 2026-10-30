import i18n from "i18next";
import { z } from "zod";

/**
 * 解析 JSON 语法错误，提取位置信息。
 *
 * 文案在校验时通过 i18n 实例取得，因此切换界面语言后会立即生效。
 */
export function parseJsonError(error: unknown): string {
  if (!(error instanceof SyntaxError)) {
    return i18n.t("providerForm.validation.configJsonInvalid", {
      defaultValue: "Config JSON format error",
    });
  }

  const message = error.message;

  // 提取位置信息：Chrome/V8: "Unexpected token ... in JSON at position 123"
  const positionMatch = message.match(/at position (\d+)/i);
  if (positionMatch) {
    const position = parseInt(positionMatch[1], 10);
    return i18n.t("providerForm.validation.jsonErrorAtPosition", {
      detail: message.split(" in JSON")[0],
      position,
      defaultValue: "JSON format error: {{detail}} (position {{position}})",
    });
  }

  // Firefox: "JSON.parse: unexpected character at line 1 column 23"
  const lineColumnMatch = message.match(/line (\d+) column (\d+)/i);
  if (lineColumnMatch) {
    return i18n.t("providerForm.validation.jsonErrorAtLineColumn", {
      line: lineColumnMatch[1],
      column: lineColumnMatch[2],
      defaultValue: "JSON format error: line {{line}}, column {{column}}",
    });
  }

  // 通用情况：保留引擎给出的错误信息
  return i18n.t("providerForm.validation.jsonError", {
    detail: message.replace(/^JSON\.parse:\s*/i, ""),
    defaultValue: "JSON format error: {{detail}}",
  });
}

export const providerSchema = z.object({
  name: z.string(), // 必填校验移至 handleSubmit 中用 toast 提示
  websiteUrl: z
    .string()
    .url({
      error: () =>
        i18n.t("providerForm.validation.invalidUrl", {
          defaultValue: "Please enter a valid URL",
        }),
    })
    .optional()
    .or(z.literal("")),
  notes: z.string().optional(),
  settingsConfig: z
    .string()
    .min(1, {
      error: () =>
        i18n.t("providerForm.fillConfigContent", {
          defaultValue: "Please fill in configuration content",
        }),
    })
    .superRefine((value, ctx) => {
      try {
        JSON.parse(value);
      } catch (error) {
        ctx.addIssue({
          code: "custom",
          message: parseJsonError(error),
        });
      }
    }),
  // 图标配置
  icon: z.string().optional(),
  iconColor: z.string().optional(),
});

export type ProviderFormData = z.infer<typeof providerSchema>;
