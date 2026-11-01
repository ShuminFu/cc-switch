import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save, Plus, Trash2, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import {
  useOpenClawAgentsDefaults,
  useSaveOpenClawAgentsDefaults,
} from "@/hooks/useOpenClaw";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { OpenClawAgentsDefaults } from "@/types";
import { useOpenClawModelOptions } from "./hooks/useOpenClawModelOptions";
import { getOpenClawTimeoutInputValue } from "./utils";
import { useEditableBuffer } from "./hooks/useEditableBuffer";
import { ExternalChangeAlert } from "./ExternalChangeAlert";

const UNSET_SENTINEL = "__unset__";

interface AgentsForm {
  /** Original section, so unknown fields survive a save. */
  defaults: OpenClawAgentsDefaults | null;
  primaryModel: string;
  fallbacks: string[];
  workspace: string;
  timeout: string;
  contextTokens: string;
  maxConcurrent: string;
}

const EMPTY_AGENTS_FORM: AgentsForm = {
  defaults: null,
  primaryModel: "",
  fallbacks: [],
  workspace: "",
  timeout: "",
  contextTokens: "",
  maxConcurrent: "",
};

// agentsData is undefined while loading, null when the section is absent.
const agentsToForm = (data: OpenClawAgentsDefaults | null): AgentsForm =>
  data
    ? {
        defaults: data,
        primaryModel: data.model?.primary ?? "",
        fallbacks: data.model?.fallbacks ?? [],
        workspace: String(data.workspace ?? ""),
        timeout: getOpenClawTimeoutInputValue(data),
        contextTokens: String(data.contextTokens ?? ""),
        maxConcurrent: String(data.maxConcurrent ?? ""),
      }
    : EMPTY_AGENTS_FORM;

const AgentsDefaultsPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: agentsData, isLoading } = useOpenClawAgentsDefaults();
  const saveAgentsMutation = useSaveOpenClawAgentsDefaults();
  const { options: modelOptions, isLoading: modelsLoading } =
    useOpenClawModelOptions();

  const { form, setForm, externalChange, reload, markSaved } =
    useEditableBuffer(agentsData, agentsToForm, EMPTY_AGENTS_FORM);
  const {
    defaults,
    primaryModel,
    fallbacks,
    workspace,
    timeout,
    contextTokens,
    maxConcurrent,
  } = form;
  const setField =
    <K extends keyof AgentsForm>(key: K) =>
    (value: AgentsForm[K]) =>
      setForm((prev) => ({ ...prev, [key]: value }));
  const setPrimaryModel = setField("primaryModel");
  const setWorkspace = setField("workspace");
  const setTimeout_ = setField("timeout");
  const setContextTokens = setField("contextTokens");
  const setMaxConcurrent = setField("maxConcurrent");
  const setFallbacks = (update: (prev: string[]) => string[]) =>
    setForm((prev) => ({ ...prev, fallbacks: update(prev.fallbacks) }));
  const [errors, setErrors] = useState<
    Partial<Record<keyof AgentsForm, string>>
  >({});

  // Build primary options, including a "not in list" entry if current value is missing
  const primaryOptions = useMemo(() => {
    const result = [...modelOptions];
    if (
      primaryModel &&
      !modelOptions.some((opt) => opt.value === primaryModel)
    ) {
      result.unshift({
        value: primaryModel,
        label: t("openclaw.agents.notInList", {
          value: primaryModel,
          defaultValue: "{{value}} (not configured)",
        }),
      });
    }
    return result;
  }, [modelOptions, primaryModel, t]);

  // For each fallback row, compute available options (exclude primary + other fallbacks)
  const getFallbackOptions = (currentIndex: number) => {
    const usedValues = new Set<string>();
    if (primaryModel) usedValues.add(primaryModel);
    fallbacks.forEach((fb, idx) => {
      if (idx !== currentIndex && fb) usedValues.add(fb);
    });

    const filtered = modelOptions.filter((opt) => !usedValues.has(opt.value));

    // If current fallback value is not in modelOptions, add a "not in list" entry
    const currentValue = fallbacks[currentIndex];
    if (
      currentValue &&
      !modelOptions.some((opt) => opt.value === currentValue)
    ) {
      filtered.unshift({
        value: currentValue,
        label: t("openclaw.agents.notInList", {
          value: currentValue,
          defaultValue: "{{value}} (not configured)",
        }),
      });
    }

    return filtered;
  };

  const handleAddFallback = () => {
    setFallbacks((prev) => [...prev, ""]);
  };

  const handleRemoveFallback = (index: number) => {
    setFallbacks((prev) => prev.filter((_, i) => i !== index));
  };

  const handleFallbackChange = (index: number, value: string) => {
    setFallbacks((prev) => {
      const next = [...prev];
      next[index] = value;
      return next;
    });
  };

  const validate = (): boolean => {
    const next: Partial<Record<keyof AgentsForm, string>> = {};
    const fallbackList = fallbacks.filter(Boolean);
    if (!primaryModel && fallbackList.length > 0) {
      next.primaryModel = t("openclaw.agents.primaryRequired", {
        defaultValue: "Choose a primary model before adding fallbacks.",
      });
    }
    const checkInt = (
      key: "timeout" | "contextTokens" | "maxConcurrent",
      min: number,
    ) => {
      const raw = form[key].trim();
      if (!raw) return;
      const n = Number(raw);
      if (!Number.isInteger(n) || n < min) {
        next[key] = t("openclaw.agents.positiveInteger", {
          min,
          defaultValue: "Enter a whole number of at least {{min}}.",
        });
      }
    };
    checkInt("timeout", 0);
    checkInt("contextTokens", 1);
    checkInt("maxConcurrent", 1);
    setErrors(next);
    return Object.keys(next).length === 0;
  };

  const handleSave = async () => {
    // Never persist an empty primary model or silently drop a mistyped
    // number (FE-UI-36).
    if (!validate()) return;
    try {
      // Preserve all unknown fields from original data
      const updated: OpenClawAgentsDefaults = { ...defaults };

      // Model configuration
      const fallbackList = fallbacks.filter(Boolean);

      if (primaryModel) {
        updated.model = {
          primary: primaryModel,
          ...(fallbackList.length > 0 ? { fallbacks: fallbackList } : {}),
        };
      }

      // Optional fields
      if (workspace.trim()) updated.workspace = workspace.trim();
      else delete updated.workspace;

      // Numeric fields: validate before saving to avoid NaN
      const parseNum = (v: string) => {
        const n = Number(v);
        return !isNaN(n) && isFinite(n) ? n : undefined;
      };

      const timeoutNum = timeout.trim() ? parseNum(timeout) : undefined;
      if (timeoutNum !== undefined) updated.timeoutSeconds = timeoutNum;
      else delete updated.timeoutSeconds;
      delete updated.timeout;

      const ctxNum = contextTokens.trim() ? parseNum(contextTokens) : undefined;
      if (ctxNum !== undefined) updated.contextTokens = ctxNum;
      else delete updated.contextTokens;

      const concNum = maxConcurrent.trim()
        ? parseNum(maxConcurrent)
        : undefined;
      if (concNum !== undefined) updated.maxConcurrent = concNum;
      else delete updated.maxConcurrent;

      await saveAgentsMutation.mutateAsync(updated);
      markSaved();
      toast.success(t("openclaw.agents.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(t("openclaw.agents.saveFailed"), {
        description: detail || undefined,
      });
    }
  };

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  const noModels = modelOptions.length === 0 && !modelsLoading;
  const hasLegacyTimeout =
    agentsData !== undefined &&
    agentsData !== null &&
    typeof agentsData.timeout === "number" &&
    typeof agentsData.timeoutSeconds !== "number";

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-6">
        {t("openclaw.agents.description")}
      </p>

      {externalChange && <ExternalChangeAlert onReload={reload} />}

      {hasLegacyTimeout && (
        <Alert className="mb-4 border-amber-500/30 bg-amber-500/5">
          <TriangleAlert className="h-4 w-4" />
          <AlertTitle>
            {t("openclaw.agents.legacyTimeoutTitle", {
              defaultValue: "Legacy timeout detected",
            })}
          </AlertTitle>
          <AlertDescription>
            {t("openclaw.agents.legacyTimeoutDescription", {
              defaultValue:
                "This config still uses agents.defaults.timeout. Saving here will migrate it to timeoutSeconds.",
            })}
          </AlertDescription>
        </Alert>
      )}

      {/* Model Configuration Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.modelSection")}
        </h3>

        <div className="space-y-4">
          {/* Primary Model */}
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.primaryModel")}
            </Label>
            {noModels ? (
              <p className="text-xs text-muted-foreground italic">
                {t("openclaw.agents.noModels", {
                  defaultValue:
                    "No configured provider models. Please add an OpenClaw provider first.",
                })}
              </p>
            ) : (
              <Select
                value={primaryModel || UNSET_SENTINEL}
                onValueChange={(v) =>
                  setPrimaryModel(v === UNSET_SENTINEL ? "" : v)
                }
              >
                <SelectTrigger className="font-mono text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={UNSET_SENTINEL}>
                    {t("openclaw.agents.notSet")}
                  </SelectItem>
                  {primaryOptions.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
            {errors.primaryModel ? (
              <p className="text-xs text-destructive mt-1" role="alert">
                {errors.primaryModel}
              </p>
            ) : (
              <p className="text-xs text-muted-foreground mt-1">
                {t("openclaw.agents.primaryModelHint")}
              </p>
            )}
          </div>

          {/* Fallback Models */}
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.fallbackModels")}
            </Label>

            {fallbacks.length === 0 && !noModels && (
              <p className="text-xs text-muted-foreground italic mb-2">
                {t("openclaw.agents.fallbackModelsHint")}
              </p>
            )}

            <div className="space-y-2">
              {fallbacks.map((fb, index) => {
                const opts = getFallbackOptions(index);
                return (
                  <div key={index} className="flex items-center gap-2">
                    <Select
                      value={fb || UNSET_SENTINEL}
                      onValueChange={(v) =>
                        handleFallbackChange(
                          index,
                          v === UNSET_SENTINEL ? "" : v,
                        )
                      }
                    >
                      <SelectTrigger className="font-mono text-xs flex-1">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={UNSET_SENTINEL}>
                          {t("openclaw.agents.notSet")}
                        </SelectItem>
                        {opts.map((opt) => (
                          <SelectItem key={opt.value} value={opt.value}>
                            {opt.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                      onClick={() => handleRemoveFallback(index)}
                      aria-label={t("common.delete")}
                    >
                      <Trash2 className="w-4 h-4" />
                    </Button>
                  </div>
                );
              })}
            </div>

            {!noModels && (
              <Button
                variant="outline"
                size="sm"
                className="mt-2"
                onClick={handleAddFallback}
              >
                <Plus className="w-4 h-4 mr-1" />
                {t("openclaw.agents.addFallback", {
                  defaultValue: "Add fallback model",
                })}
              </Button>
            )}
          </div>
        </div>
      </div>

      {/* Runtime Parameters Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.runtimeSection")}
        </h3>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.workspace")}
            </Label>
            <Input
              value={workspace}
              onChange={(e) => setWorkspace(e.target.value)}
              placeholder="~/projects"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.timeout")}
            </Label>
            <Input
              type="number"
              min={0}
              step={1}
              value={timeout}
              onChange={(e) => setTimeout_(e.target.value)}
              placeholder="300"
              className="font-mono text-xs"
              aria-label={t("openclaw.agents.timeout")}
              aria-invalid={!!errors.timeout}
            />
            {errors.timeout && (
              <p className="text-xs text-destructive mt-1" role="alert">
                {errors.timeout}
              </p>
            )}
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.contextTokens")}
            </Label>
            <Input
              type="number"
              min={1}
              step={1}
              value={contextTokens}
              onChange={(e) => setContextTokens(e.target.value)}
              placeholder="200000"
              className="font-mono text-xs"
              aria-label={t("openclaw.agents.contextTokens")}
              aria-invalid={!!errors.contextTokens}
            />
            {errors.contextTokens && (
              <p className="text-xs text-destructive mt-1" role="alert">
                {errors.contextTokens}
              </p>
            )}
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.maxConcurrent")}
            </Label>
            <Input
              type="number"
              min={1}
              step={1}
              value={maxConcurrent}
              onChange={(e) => setMaxConcurrent(e.target.value)}
              placeholder="4"
              className="font-mono text-xs"
              aria-label={t("openclaw.agents.maxConcurrent")}
              aria-invalid={!!errors.maxConcurrent}
            />
            {errors.maxConcurrent && (
              <p className="text-xs text-destructive mt-1" role="alert">
                {errors.maxConcurrent}
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Save button */}
      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveAgentsMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveAgentsMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default AgentsDefaultsPanel;
