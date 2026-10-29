import { appVariant } from "../baseSchema";

/**
 * OpenClaw 在判别联合中的分支。应用专属的编辑状态（见 useOpenclawFormState）
 * 直接读写 settingsConfig，不是 react-hook-form 字段，所以这里只有基础字段。
 */
export const openclawProviderFormSchema = appVariant("openclaw");
