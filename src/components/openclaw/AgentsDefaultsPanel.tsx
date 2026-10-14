import React, { useMemo } from "react";
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
import { useEditableResource } from "@/hooks/useEditableResource";
import { ExternalChangeBanner } from "@/components/common/ExternalChangeBanner";
import { parseStrictNonNegativeInt } from "@/utils/numberInput";

const UNSET_SENTINEL = "__unset__";

interface AgentsDraft {
  primaryModel: string;
  fallbacks: string[];
  workspace: string;
  timeout: string;
  contextTokens: string;
  maxConcurrent: string;
}

const EMPTY_AGENTS_DRAFT: AgentsDraft = {
  primaryModel: "",
  fallbacks: [],
  workspace: "",
  timeout: "",
  contextTokens: "",
  maxConcurrent: "",
};

// agents.defaults is null when the config section is absent.
const toAgentsDraft = (data: OpenClawAgentsDefaults | null): AgentsDraft =>
  data
    ? {
        primaryModel: data.model?.primary ?? "",
        fallbacks: data.model?.fallbacks ?? [],
        workspace: String(data.workspace ?? ""),
        timeout: getOpenClawTimeoutInputValue(data),
        contextTokens: String(data.contextTokens ?? ""),
        maxConcurrent: String(data.maxConcurrent ?? ""),
      }
    : EMPTY_AGENTS_DRAFT;

const AgentsDefaultsPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: agentsData, isLoading } = useOpenClawAgentsDefaults();
  const saveAgentsMutation = useSaveOpenClawAgentsDefaults();
  const { options: modelOptions, isLoading: modelsLoading } =
    useOpenClawModelOptions();

  // Refetches never clobber unsaved edits; an on-disk change while dirty
  // shows a Reload / Keep mine banner. `baselineData` keeps unknown fields
  // from the version the form is based on.
  const {
    draft,
    setDraft,
    baselineData,
    externalChange,
    reload,
    keepMine,
    markSaved,
  } = useEditableResource<OpenClawAgentsDefaults | null, AgentsDraft>({
    data: agentsData,
    toDraft: toAgentsDraft,
    initialDraft: EMPTY_AGENTS_DRAFT,
  });
  const defaults = baselineData ?? null;
  const {
    primaryModel,
    fallbacks,
    workspace,
    timeout,
    contextTokens,
    maxConcurrent,
  } = draft;

  const setDraftField =
    <K extends keyof AgentsDraft>(key: K) =>
    (action: React.SetStateAction<AgentsDraft[K]>) =>
      setDraft((prev) => ({
        ...prev,
        [key]:
          typeof action === "function"
            ? (action as (p: AgentsDraft[K]) => AgentsDraft[K])(prev[key])
            : action,
      }));
  const setPrimaryModel = setDraftField("primaryModel");
  const setFallbacks = setDraftField("fallbacks");
  const setWorkspace = setDraftField("workspace");
  const setTimeout_ = setDraftField("timeout");
  const setContextTokens = setDraftField("contextTokens");
  const setMaxConcurrent = setDraftField("maxConcurrent");

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

  const handleSave = async () => {
    const snapshot = draft;
    // Numeric fields must be non-negative whole numbers. Reject instead of
    // silently saving a decimal/negative value or dropping the field.
    const numericFields = [
      { raw: timeout, labelKey: "openclaw.agents.timeout" },
      { raw: contextTokens, labelKey: "openclaw.agents.contextTokens" },
      { raw: maxConcurrent, labelKey: "openclaw.agents.maxConcurrent" },
    ] as const;
    const [timeoutNum, ctxNum, concNum] = numericFields.map(({ raw }) =>
      parseStrictNonNegativeInt(raw),
    );
    const invalidIndex = [timeoutNum, ctxNum, concNum].indexOf(null);
    if (invalidIndex !== -1) {
      toast.error(
        t("openclaw.agents.invalidInteger", {
          field: t(numericFields[invalidIndex].labelKey),
        }),
      );
      return;
    }

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
      } else if (fallbackList.length > 0) {
        updated.model = { primary: "", fallbacks: fallbackList };
      }

      // Optional fields
      if (workspace.trim()) updated.workspace = workspace.trim();
      else delete updated.workspace;

      // Numeric fields (validated above; null already rejected)
      if (timeoutNum != null) updated.timeoutSeconds = timeoutNum;
      else delete updated.timeoutSeconds;
      delete updated.timeout;

      if (ctxNum != null) updated.contextTokens = ctxNum;
      else delete updated.contextTokens;

      if (concNum != null) updated.maxConcurrent = concNum;
      else delete updated.maxConcurrent;

      await saveAgentsMutation.mutateAsync(updated);
      markSaved(snapshot);
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

      {externalChange && (
        <ExternalChangeBanner
          className="mb-4"
          onReload={reload}
          onKeepMine={keepMine}
        />
      )}

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
                onValueChange={(v) => {
                  setPrimaryModel(v === UNSET_SENTINEL ? "" : v);
                }}
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
            <p className="text-xs text-muted-foreground mt-1">
              {t("openclaw.agents.primaryModelHint")}
            </p>
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
              onChange={(e) => {
                setWorkspace(e.target.value);
              }}
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
              inputMode="numeric"
              value={timeout}
              onChange={(e) => {
                setTimeout_(e.target.value);
              }}
              placeholder="300"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.contextTokens")}
            </Label>
            <Input
              type="number"
              min={0}
              step={1}
              inputMode="numeric"
              value={contextTokens}
              onChange={(e) => {
                setContextTokens(e.target.value);
              }}
              placeholder="200000"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.maxConcurrent")}
            </Label>
            <Input
              type="number"
              min={0}
              step={1}
              inputMode="numeric"
              value={maxConcurrent}
              onChange={(e) => {
                setMaxConcurrent(e.target.value);
              }}
              placeholder="4"
              className="font-mono text-xs"
            />
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
