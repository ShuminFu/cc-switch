import { appVariant } from "../baseSchema";

/**
 * OpenCode 在判别联合中的分支。应用专属的编辑状态（见 useOpencodeFormState、
 * useOmoDraftState）不是 react-hook-form 字段，所以这里只有基础字段。
 */
export const opencodeProviderFormSchema = appVariant("opencode");
