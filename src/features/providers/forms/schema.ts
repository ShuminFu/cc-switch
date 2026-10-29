import { z } from "zod";
import { zodResolver } from "@hookform/resolvers/zod";
import type { Resolver } from "react-hook-form";
import type { AppId } from "@/lib/api";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";

/**
 * 供应商表单的基础 schema：所有应用共享的字段（名称、网址、备注、
 * settingsConfig、图标）。
 */
export const baseProviderFormSchema = providerSchema;

const appVariant = <A extends AppId>(appId: A) =>
  baseProviderFormSchema.extend({ appId: z.literal(appId) });

/**
 * 每个应用一个分支，以 `appId` 区分。目前各分支只有基础字段；
 * 拆分各应用表单模块时，应用专属字段加在对应分支上。
 */
export const providerFormSchemas = {
  claude: appVariant("claude"),
  "claude-desktop": appVariant("claude-desktop"),
  codex: appVariant("codex"),
  gemini: appVariant("gemini"),
  grokbuild: appVariant("grokbuild"),
  opencode: appVariant("opencode"),
  openclaw: appVariant("openclaw"),
  hermes: appVariant("hermes"),
} satisfies { [A in AppId]: ReturnType<typeof appVariant<A>> };

export const providerFormSchema = z.discriminatedUnion("appId", [
  providerFormSchemas.claude,
  providerFormSchemas["claude-desktop"],
  providerFormSchemas.codex,
  providerFormSchemas.gemini,
  providerFormSchemas.grokbuild,
  providerFormSchemas.opencode,
  providerFormSchemas.openclaw,
  providerFormSchemas.hermes,
]);

export type ProviderFormSchemaValues = z.infer<typeof providerFormSchema>;

const unionResolver = zodResolver(providerFormSchema);

/**
 * 用判别联合校验某个应用的表单值。
 * `appId` 不是表单字段：校验前注入、校验后剥离，提交的 payload 不含它。
 */
export function providerFormResolver(appId: AppId): Resolver<ProviderFormData> {
  return async (values, context, options) => {
    const result = await unionResolver(
      { ...values, appId },
      context,
      options as unknown as Parameters<typeof unionResolver>[2],
    );
    if (Object.keys(result.errors).length > 0) {
      return { values: {}, errors: result.errors };
    }
    const { appId: _appId, ...data } =
      result.values as ProviderFormSchemaValues;
    return { values: data, errors: {} };
  };
}
