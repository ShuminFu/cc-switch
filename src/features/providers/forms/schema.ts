import { z } from "zod";
import { zodResolver } from "@hookform/resolvers/zod";
import type { Resolver } from "react-hook-form";
import type { AppId } from "@/lib/api";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { appVariant } from "./baseSchema";
import { claudeProviderFormSchema } from "./claude/schema";
import { claudeDesktopProviderFormSchema } from "./claude-desktop/schema";
import { codexProviderFormSchema } from "./codex/schema";
import { geminiProviderFormSchema } from "./gemini/schema";
import { grokBuildProviderFormSchema } from "./grokbuild/schema";
import { hermesProviderFormSchema } from "./hermes/schema";
import { openclawProviderFormSchema } from "./openclaw/schema";
import { opencodeProviderFormSchema } from "./opencode/schema";

export { baseProviderFormSchema } from "./baseSchema";

/**
 * 每个应用一个分支，以 `appId` 区分。各应用从 `<app>/schema.ts` 取分支；
 * 应用专属字段加在对应分支上。
 */
export const providerFormSchemas = {
  claude: claudeProviderFormSchema,
  "claude-desktop": claudeDesktopProviderFormSchema,
  codex: codexProviderFormSchema,
  gemini: geminiProviderFormSchema,
  grokbuild: grokBuildProviderFormSchema,
  opencode: opencodeProviderFormSchema,
  openclaw: openclawProviderFormSchema,
  hermes: hermesProviderFormSchema,
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
