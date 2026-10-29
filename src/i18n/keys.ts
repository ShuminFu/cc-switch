import type { ParseKeys } from "i18next";

/**
 * en.json 中所有合法的翻译 key（类型由 i18next.d.ts 中的 CustomTypeOptions 推导）。
 * 运行时才确定的 key（后端返回的错误码、预设数据中的字段等）在使用前须先用
 * i18n.exists() 校验，再断言为该类型，不允许再依赖 defaultValue 兜底。
 */
export type TranslationKey = ParseKeys<"translation">;
