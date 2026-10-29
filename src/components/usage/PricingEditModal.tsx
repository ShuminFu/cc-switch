import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save, Plus, Globe } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useUpdateModelPricing } from "@/lib/query/usage";
import { isNonNegativeDecimalString, type ModelPricing } from "@/types/usage";
import { ModelsDevPickerDialog } from "./ModelsDevPickerDialog";

interface PricingEditModalProps {
  open: boolean;
  model: ModelPricing;
  isNew?: boolean;
  onClose: () => void;
}

const PRICE_INPUT_STEP = "0.0001";

export function PricingEditModal({
  open,
  model,
  isNew = false,
  onClose,
}: PricingEditModalProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();
  const [isPickerOpen, setIsPickerOpen] = useState(false);

  const [formData, setFormData] = useState({
    modelId: model.modelId,
    displayName: model.displayName,
    inputCost: model.inputCostPerMillion,
    outputCost: model.outputCostPerMillion,
    cacheReadCost: model.cacheReadCostPerMillion,
    cacheCreationCost: model.cacheCreationCostPerMillion,
  });

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // 验证模型 ID
    if (isNew && !formData.modelId.trim()) {
      toast.error(t("usage.modelIdRequired"));
      return;
    }

    // 验证非负数
    const values = [
      formData.inputCost,
      formData.outputCost,
      formData.cacheReadCost,
      formData.cacheCreationCost,
    ];

    for (const value of values) {
      if (!isNonNegativeDecimalString(value)) {
        toast.error(t("usage.invalidPrice"));
        return;
      }
    }

    try {
      await updatePricing.mutateAsync({
        modelId: isNew ? formData.modelId : model.modelId,
        displayName: formData.displayName,
        inputCost: formData.inputCost,
        outputCost: formData.outputCost,
        cacheReadCost: formData.cacheReadCost,
        cacheCreationCost: formData.cacheCreationCost,
      });

      toast.success(
        isNew ? t("usage.pricingAdded") : t("usage.pricingUpdated"),
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
          ? t("usage.addPricing")
          : `${t("usage.editPricing")} - ${model.modelId}`
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
            ? t("common.saving")
            : isNew
              ? t("common.add")
              : t("common.save")}
        </Button>
      }
    >
      {isNew && (
        <div className="mb-6 flex items-center justify-between gap-3 rounded-md border border-border/50 bg-muted/20 px-3 py-2.5">
          <p className="text-xs text-muted-foreground">
            {t("usage.modelsDevHint")}
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setIsPickerOpen(true)}
            className="shrink-0"
          >
            <Globe className="mr-1.5 h-4 w-4" />
            {t("usage.importFromModelsDev")}
          </Button>
        </div>
      )}

      <form id="pricing-form" onSubmit={handleSubmit} className="space-y-6">
        {isNew && (
          <div className="space-y-2">
            <Label htmlFor="modelId">{t("usage.modelId")}</Label>
            <Input
              id="modelId"
              value={formData.modelId}
              onChange={(e) =>
                setFormData({ ...formData, modelId: e.target.value })
              }
              placeholder={t("usage.modelIdPlaceholder")}
              required
            />
          </div>
        )}

        <div className="space-y-2">
          <Label htmlFor="displayName">{t("usage.displayName")}</Label>
          <Input
            id="displayName"
            value={formData.displayName}
            onChange={(e) =>
              setFormData({ ...formData, displayName: e.target.value })
            }
            placeholder={t("usage.displayNamePlaceholder")}
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="inputCost">{t("usage.inputCostPerMillion")}</Label>
          <Input
            id="inputCost"
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.inputCost}
            onChange={(e) =>
              setFormData({ ...formData, inputCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="outputCost">{t("usage.outputCostPerMillion")}</Label>
          <Input
            id="outputCost"
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.outputCost}
            onChange={(e) =>
              setFormData({ ...formData, outputCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheReadCost">
            {t("usage.cacheReadCostPerMillion")}
          </Label>
          <Input
            id="cacheReadCost"
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.cacheReadCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheReadCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheCreationCost">
            {t("usage.cacheCreationCostPerMillion")}
          </Label>
          <Input
            id="cacheCreationCost"
            type="number"
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.cacheCreationCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheCreationCost: e.target.value })
            }
            required
          />
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
