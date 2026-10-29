import type { ComponentType } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderFormProps } from "@/features/providers/forms";
import { ClaudeProviderForm } from "@/features/providers/forms/claude/ClaudeProviderForm";
import { ClaudeDesktopProviderForm } from "@/features/providers/forms/claude-desktop/ClaudeDesktopProviderForm";
import { CodexProviderForm } from "@/features/providers/forms/codex/CodexProviderForm";
import { GeminiProviderForm } from "@/features/providers/forms/gemini/GeminiProviderForm";
import { GrokBuildProviderForm } from "@/features/providers/forms/grokbuild/GrokBuildProviderForm";
import { ProviderFormFull } from "./ProviderFormFull";

export type {
  ProviderFormProps,
  ProviderFormValues,
} from "@/features/providers/forms";
export {
  normalizeCodexCatalogModelsForSave,
  parseJsonObjectForSave,
} from "@/features/providers/forms";

/**
 * 每个应用对应的编辑器。键覆盖全部 AppId，新增应用时类型检查会要求在此登记。
 * 所有编辑器都渲染在 ProviderFormShell 内：共享基础 schema 与判别联合校验、
 * 内联错误汇总（有错误时阻止保存）和"仍要保存"确认框。
 * 各应用模块在 features/providers/forms/<app>/（schema、fromProvider、
 * toSettingsConfig、Fields），预设表按需加载。
 */
export const PROVIDER_FORM_EDITORS: Record<
  AppId,
  ComponentType<ProviderFormProps>
> = {
  claude: ClaudeProviderForm,
  "claude-desktop": ClaudeDesktopProviderForm,
  codex: CodexProviderForm,
  gemini: GeminiProviderForm,
  grokbuild: GrokBuildProviderForm,
  opencode: ProviderFormFull,
  openclaw: ProviderFormFull,
  hermes: ProviderFormFull,
};

export function ProviderForm(props: ProviderFormProps) {
  const Editor = PROVIDER_FORM_EDITORS[props.appId];
  return <Editor {...props} />;
}
