import { appVariant } from "../baseSchema";

/**
 * Claude 在判别联合中的分支。应用专属的编辑状态（见 fromProvider）目前不是
 * react-hook-form 字段，所以这里只有基础字段。
 */
export const claudeProviderFormSchema = appVariant("claude");
