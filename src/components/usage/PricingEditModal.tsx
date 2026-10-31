import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save, Plus, Globe } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useModelPricing, useUpdateModelPricing } from "@/lib/query/usage";
import { isNonNegativeDecimalString, type ModelPricing } from "@/types/usage";
import {
  ModelsDevPickerDialog,
  normalizeModelIdForPricing,
} from "./ModelsDevPickerDialog";

interface PricingEditModalProps {
  open: boolean;
  model: ModelPricing;
  isNew?: boolean;
  onClose: () => void;
}

const PRICE_INPUT_STEP = "0.0001";

type PriceField =
  | "inputCost"
  | "outputCost"
  | "cacheReadCost"
  | "cacheCreationCost";
const PRICE_FIELDS: PriceField[] = [
  "inputCost",
  "outputCost",
  "cacheReadCost",
  "cacheCreationCost",
];
type FieldErrors = Partial<Record<"modelId" | PriceField, string>>;

export function PricingEditModal({
  open,
  model,
  isNew = false,
  onClose,
}: PricingEditModalProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();
  const { data: existingPricing } = useModelPricing();
  const [isPickerOpen, setIsPickerOpen] = useState(false);
  const [errors, setErrors] = useState<FieldErrors>({});

  const [formData, setFormData] = useState({
    modelId: model.modelId,
    displayName: model.displayName,
    inputCost: model.inputCostPerMillion,
    outputCost: model.outputCostPerMillion,
    cacheReadCost: model.cacheReadCostPerMillion,
    cacheCreationCost: model.cacheCreationCostPerMillion,
  });

  const normalizedModelId = normalizeModelIdForPricing(formData.modelId);
  const isDuplicate =
    isNew &&
    !!normalizedModelId &&
    (existingPricing ?? []).some(
      (p) => p.modelId.toLowerCase() === normalizedModelId,
    );

  const fieldError = (field: keyof FieldErrors) =>
    errors[field] ? (
      <p
        id={`${field}-error`}
        className="text-xs text-destructive"
        data-testid={`${field}-error`}
      >
        {errors[field]}
      </p>
    ) : null;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // Field-level validation (FE-UI-30). New model IDs are normalised the
    // same way the backend matches usage rows, so a typed
    // `openai/gpt-4o` is stored as `gpt-4o` and can actually be attributed.
    const nextErrors: FieldErrors = {};
    if (isNew) {
      if (!normalizedModelId) {
        nextErrors.modelId = t("usage.modelIdRequired", "模型 ID 不能为空");
      } else if (isDuplicate) {
        nextErrors.modelId = t("usage.modelIdDuplicate", {
          defaultValue: "Pricing for {{id}} already exists.",
          id: normalizedModelId,
        });
      }
    }
    for (const field of PRICE_FIELDS) {
      if (!isNonNegativeDecimalString(formData[field])) {
        nextErrors[field] = t("usage.invalidPrice", "价格必须为非负数");
      }
    }
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) {
      return;
    }

    try {
      await updatePricing.mutateAsync({
        modelId: isNew ? normalizedModelId : model.modelId,
        displayName: formData.displayName,
        inputCost: formData.inputCost,
        outputCost: formData.outputCost,
        cacheReadCost: formData.cacheReadCost,
        cacheCreationCost: formData.cacheCreationCost,
      });

      toast.success(
        isNew
          ? t("usage.pricingAdded", "定价已添加")
          : t("usage.pricingUpdated", "定价已更新"),
        { closeButton: true },
      );

      onClose();
    } catch (error) {
      toast.error(String(error));
    }
  };

  return (
    <FullScreenPanel
      isOpen={open}
      title={
        isNew
          ? t("usage.addPricing", "新增定价")
          : `${t("usage.editPricing", "编辑定价")} - ${model.modelId}`
      }
      onClose={onClose}
      footer={
        <Button
          type="submit"
          form="pricing-form"
          disabled={updatePricing.isPending}
        >
          {isNew ? (
            <Plus className="h-4 w-4 mr-2" />
          ) : (
            <Save className="h-4 w-4 mr-2" />
          )}
          {updatePricing.isPending
            ? t("common.saving", "保存中...")
            : isNew
              ? t("common.add", "新增")
              : t("common.save", "保存")}
        </Button>
      }
    >
      {isNew && (
        <div className="mb-6 flex items-center justify-between gap-3 rounded-md border border-border/50 bg-muted/20 px-3 py-2.5">
          <p className="text-xs text-muted-foreground">
            {t(
              "usage.modelsDevHint",
              "无需手动填写，可从 models.dev 选择模型定价",
            )}
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setIsPickerOpen(true)}
            className="shrink-0"
          >
            <Globe className="mr-1.5 h-4 w-4" />
            {t("usage.importFromModelsDev", "从 models.dev 导入")}
          </Button>
        </div>
      )}

      <form id="pricing-form" onSubmit={handleSubmit} className="space-y-6">
        {isNew && (
          <div className="space-y-2">
            <Label htmlFor="modelId">{t("usage.modelId", "模型 ID")}</Label>
            <Input
              id="modelId"
              value={formData.modelId}
              onChange={(e) =>
                setFormData({ ...formData, modelId: e.target.value })
              }
              placeholder={t("usage.modelIdPlaceholder", {
                defaultValue: "例如: claude-3-5-sonnet-20241022",
              })}
              aria-invalid={!!errors.modelId}
              required
            />
            {normalizedModelId &&
              normalizedModelId !== formData.modelId.trim() && (
                <p
                  className="text-xs text-muted-foreground"
                  data-testid="modelId-normalized"
                >
                  {t("usage.modelIdNormalizedHint", {
                    defaultValue: "Will be saved as {{id}}",
                    id: normalizedModelId,
                  })}
                </p>
              )}
            {fieldError("modelId")}
          </div>
        )}

        <div className="space-y-2">
          <Label htmlFor="displayName">
            {t("usage.displayName", "显示名称")}
          </Label>
          <Input
            id="displayName"
            value={formData.displayName}
            onChange={(e) =>
              setFormData({ ...formData, displayName: e.target.value })
            }
            placeholder={t("usage.displayNamePlaceholder", {
              defaultValue: "例如: Claude 3.5 Sonnet",
            })}
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="inputCost">
            {t("usage.inputCostPerMillion", "输入成本 (每百万 tokens, USD)")}
          </Label>
          <Input
            id="inputCost"
            aria-invalid={!!errors.inputCost}
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.inputCost}
            onChange={(e) =>
              setFormData({ ...formData, inputCost: e.target.value })
            }
            required
          />
          {fieldError("inputCost")}
        </div>

        <div className="space-y-2">
          <Label htmlFor="outputCost">
            {t("usage.outputCostPerMillion", "输出成本 (每百万 tokens, USD)")}
          </Label>
          <Input
            id="outputCost"
            aria-invalid={!!errors.outputCost}
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.outputCost}
            onChange={(e) =>
              setFormData({ ...formData, outputCost: e.target.value })
            }
            required
          />
          {fieldError("outputCost")}
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheReadCost">
            {t(
              "usage.cacheReadCostPerMillion",
              "缓存读取成本 (每百万 tokens, USD)",
            )}
          </Label>
          <Input
            id="cacheReadCost"
            aria-invalid={!!errors.cacheReadCost}
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.cacheReadCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheReadCost: e.target.value })
            }
            required
          />
          {fieldError("cacheReadCost")}
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheCreationCost">
            {t(
              "usage.cacheCreationCostPerMillion",
              "缓存写入成本 (每百万 tokens, USD)",
            )}
          </Label>
          <Input
            id="cacheCreationCost"
            aria-invalid={!!errors.cacheCreationCost}
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.cacheCreationCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheCreationCost: e.target.value })
            }
            required
          />
          {fieldError("cacheCreationCost")}
        </div>
      </form>

      {isNew && isPickerOpen && (
        <ModelsDevPickerDialog
          open={isPickerOpen}
          onClose={() => setIsPickerOpen(false)}
          onImported={() => {
            setIsPickerOpen(false);
            onClose();
          }}
        />
      )}
    </FullScreenPanel>
  );
}
