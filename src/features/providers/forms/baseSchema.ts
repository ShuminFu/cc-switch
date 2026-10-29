import { z } from "zod";
import type { AppId } from "@/lib/api";
import { providerSchema } from "@/lib/schemas/provider";

/**
 * 供应商表单的基础 schema：所有应用共享的字段（名称、网址、备注、
 * settingsConfig、图标）。
 */
export const baseProviderFormSchema = providerSchema;

/** 判别联合中某个应用的分支：基础字段 + `appId` 字面量 */
export const appVariant = <A extends AppId>(appId: A) =>
  baseProviderFormSchema.extend({ appId: z.literal(appId) });
