import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
  type SyntheticEvent,
} from "react";
import {
  useForm,
  useFormState,
  type DefaultValues,
  type UseFormReturn,
} from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { AppId } from "@/lib/api";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { providerFormResolver } from "./schema";
import type { SaveGate } from "./useSaveGate";
import { ProviderFormErrorSummary } from "./ProviderFormErrorSummary";

/** 对话框底部的保存按钮通过 form="provider-form" 关联到表单 */
export const PROVIDER_FORM_ID = "provider-form";

/** 带此属性的元素（如预设搜索框）内的输入不代表配置修改，不计入未保存修改 */
export const UNSAVED_CHANGES_IGNORE_ATTR = "data-unsaved-changes-ignore";

/**
 * 由 ProviderForm 分发器注入的"未保存修改"回调（来自 ProviderFormProps.onDirtyChange），
 * 外壳统一上报，各应用编辑器无需逐个透传。
 */
export const ProviderFormDirtyContext = createContext<
  ((isDirty: boolean) => void) | undefined
>(undefined);

/** 各编辑器共用的 react-hook-form 实例：按应用走判别联合 schema 校验 */
export function useProviderForm(
  appId: AppId,
  defaultValues: DefaultValues<ProviderFormData>,
): UseFormReturn<ProviderFormData> {
  const resolver = useMemo(() => providerFormResolver(appId), [appId]);
  return useForm<ProviderFormData>({
    resolver,
    defaultValues,
    mode: "onSubmit",
  });
}

interface ProviderFormShellProps {
  form: UseFormReturn<ProviderFormData>;
  gate: SaveGate;
  /** schema 校验通过后调用；需要阻止保存时调用 gate.block() 并返回 */
  onSubmit: (values: ProviderFormData) => Promise<void> | void;
  className?: string;
  showButtons: boolean;
  submitLabel: string;
  onCancel: () => void;
  children: ReactNode;
}

/**
 * 供应商表单外壳：提供 FormProvider（字段组件用 useFormContext 读写），
 * 渲染 <form>、内联错误汇总、操作按钮和"仍要保存"确认框。
 * 各应用编辑器只负责字段与组装 payload。
 */
export function ProviderFormShell({
  form,
  gate,
  onSubmit,
  className,
  showButtons,
  submitLabel,
  onCancel,
  children,
}: ProviderFormShellProps) {
  const { t } = useTranslation();
  const { isSubmitting, isDirty } = useFormState({ control: form.control });

  // 未保存修改 = RHF 字段已变更，或用户在表单内直接编辑过不受 RHF 管理的编辑器
  // （Codex/Gemini 配置、API Key 等由各自 hooks 持有状态，setValue 不会标记 dirty）。
  // 只统计表单 DOM 内部的输入事件：通用配置等弹窗经 portal 冒泡上来但单独保存，不算。
  const formRef = useRef<HTMLFormElement>(null);
  const [hasEditorEdits, setHasEditorEdits] = useState(false);
  const markEditorEdited = (event: SyntheticEvent) => {
    const target = event.target as Element;
    if (
      formRef.current?.contains(target) &&
      !target.closest?.(`[${UNSAVED_CHANGES_IGNORE_ATTR}]`)
    ) {
      setHasEditorEdits(true);
    }
  };
  const onDirtyChange = useContext(ProviderFormDirtyContext);
  const onDirtyChangeRef = useRef(onDirtyChange);
  onDirtyChangeRef.current = onDirtyChange;
  const hasUnsavedChanges = isDirty || hasEditorEdits;
  useEffect(() => {
    onDirtyChangeRef.current?.(hasUnsavedChanges);
  }, [hasUnsavedChanges]);
  // 表单卸载（关闭面板、切换标签页）后修改已不存在
  useEffect(() => () => onDirtyChangeRef.current?.(false), []);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    // 每次提交都重新检查，上一次的阻断错误不保留
    gate.clearBlocking();
    return form.handleSubmit(onSubmit)(event);
  };

  return (
    <>
      <Form {...form}>
        <form
          ref={formRef}
          id={PROVIDER_FORM_ID}
          onSubmit={handleSubmit}
          onInput={markEditorEdited}
          onChange={markEditorEdited}
          className={className}
        >
          {children}

          <ProviderFormErrorSummary errors={gate.blockingErrors} />

          {showButtons && (
            <div className="flex justify-end gap-2">
              <Button variant="outline" type="button" onClick={onCancel}>
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                disabled={isSubmitting || gate.isConfirmSubmitting}
              >
                {submitLabel}
              </Button>
            </div>
          )}
        </form>
      </Form>

      <ConfirmDialog
        isOpen={gate.softIssues !== null && gate.softIssues.length > 0}
        variant="info"
        title={t("providerForm.softValidation.title")}
        message={
          (gate.softIssues ?? []).map((issue) => `• ${issue}`).join("\n") +
          "\n\n" +
          t("providerForm.softValidation.hint")
        }
        confirmText={t("providerForm.softValidation.saveAnyway")}
        cancelText={t("common.cancel")}
        onConfirm={() => void gate.confirm()}
        onCancel={gate.cancelConfirm}
      />
    </>
  );
}
