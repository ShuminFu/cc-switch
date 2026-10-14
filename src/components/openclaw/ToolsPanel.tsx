import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Save, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { useOpenClawTools, useSaveOpenClawTools } from "@/hooks/useOpenClaw";
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
import type { OpenClawToolsConfig, OpenClawToolsProfile } from "@/types";
import {
  getOpenClawToolsProfileSelectValue,
  getOpenClawUnsupportedProfile,
  OPENCLAW_TOOL_PROFILES,
  OPENCLAW_UNSET_PROFILE,
  OPENCLAW_UNSUPPORTED_PROFILE,
} from "./utils";
import {
  defaultDraftEquals,
  useEditableResource,
} from "@/hooks/useEditableResource";
import { ExternalChangeBanner } from "@/components/common/ExternalChangeBanner";

interface ListItem {
  id: string;
  value: string;
}

interface ToolsDraft {
  config: OpenClawToolsConfig;
  allow: ListItem[];
  deny: ListItem[];
}

const toListItems = (values: string[] | undefined): ListItem[] =>
  (values ?? []).map((value) => ({ id: crypto.randomUUID(), value }));

const toToolsDraft = (data: OpenClawToolsConfig): ToolsDraft => ({
  config: data,
  allow: toListItems(data.allow),
  deny: toListItems(data.deny),
});

// Row ids are regenerated on every seed, so compare by values only.
const toolsDraftEquals = (a: ToolsDraft, b: ToolsDraft) =>
  defaultDraftEquals(
    {
      config: a.config,
      allow: a.allow.map((i) => i.value),
      deny: a.deny.map((i) => i.value),
    },
    {
      config: b.config,
      allow: b.allow.map((i) => i.value),
      deny: b.deny.map((i) => i.value),
    },
  );

const ToolsPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: toolsData, isLoading } = useOpenClawTools();
  const saveToolsMutation = useSaveOpenClawTools();
  // Refetches never clobber unsaved edits; an on-disk change while dirty
  // shows a Reload / Keep mine banner.
  const { draft, setDraft, externalChange, reload, keepMine, markSaved } =
    useEditableResource<OpenClawToolsConfig, ToolsDraft>({
      data: toolsData ?? undefined,
      toDraft: toToolsDraft,
      initialDraft: { config: {}, allow: [], deny: [] },
      isEqual: toolsDraftEquals,
    });
  const { config, allow: allowList, deny: denyList } = draft;

  const setDraftField =
    <K extends keyof ToolsDraft>(key: K) =>
    (action: React.SetStateAction<ToolsDraft[K]>) =>
      setDraft((prev) => ({
        ...prev,
        [key]:
          typeof action === "function"
            ? (action as (p: ToolsDraft[K]) => ToolsDraft[K])(prev[key])
            : action,
      }));
  const setConfig = setDraftField("config");
  const setAllowList = setDraftField("allow");
  const setDenyList = setDraftField("deny");

  const unsupportedProfile = getOpenClawUnsupportedProfile(config.profile);

  const profileLabels = useMemo<Record<OpenClawToolsProfile, string>>(
    () => ({
      minimal: t("openclaw.tools.profileMinimal", {
        defaultValue: "Minimal",
      }),
      coding: t("openclaw.tools.profileCoding", {
        defaultValue: "Coding",
      }),
      messaging: t("openclaw.tools.profileMessaging", {
        defaultValue: "Messaging",
      }),
      full: t("openclaw.tools.profileFull", {
        defaultValue: "Full",
      }),
    }),
    [t],
  );

  const handleSave = async () => {
    const snapshot = draft;
    try {
      const { profile, allow, deny, ...other } = config;
      const newConfig: OpenClawToolsConfig = {
        ...other,
        profile,
        allow: allowList.map((item) => item.value).filter((s) => s.trim()),
        deny: denyList.map((item) => item.value).filter((s) => s.trim()),
      };

      await saveToolsMutation.mutateAsync(newConfig);
      markSaved(snapshot);
      toast.success(t("openclaw.tools.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(t("openclaw.tools.saveFailed"), {
        description: detail || undefined,
      });
    }
  };

  const updateListItem = (
    setList: React.Dispatch<React.SetStateAction<ListItem[]>>,
    index: number,
    value: string,
  ) => {
    setList((prev) =>
      prev.map((item, i) => (i === index ? { ...item, value } : item)),
    );
  };

  const removeListItem = (
    setList: React.Dispatch<React.SetStateAction<ListItem[]>>,
    index: number,
  ) => {
    setList((prev) => prev.filter((_, i) => i !== index));
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

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-6">
        {t("openclaw.tools.description")}
      </p>

      {externalChange && (
        <ExternalChangeBanner
          className="mb-6"
          onReload={reload}
          onKeepMine={keepMine}
        />
      )}

      {unsupportedProfile && (
        <Alert className="mb-6 border-amber-500/30 bg-amber-500/5">
          <TriangleAlert className="h-4 w-4" />
          <AlertTitle>
            {t("openclaw.tools.unsupportedProfileTitle", {
              defaultValue: "Unsupported tools profile",
            })}
          </AlertTitle>
          <AlertDescription>
            {t("openclaw.tools.unsupportedProfileDescription", {
              value: unsupportedProfile,
              defaultValue:
                "The current tools.profile value '{{value}}' is not in the supported OpenClaw list. It will be preserved until you choose a new value.",
            })}
          </AlertDescription>
        </Alert>
      )}

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.profile")}</Label>
        <Select
          value={getOpenClawToolsProfileSelectValue(config.profile)}
          onValueChange={(value) => {
            if (value === OPENCLAW_UNSUPPORTED_PROFILE) return;
            if (value === OPENCLAW_UNSET_PROFILE) {
              setConfig((prev) => ({ ...prev, profile: undefined }));
              return;
            }
            setConfig((prev) => ({ ...prev, profile: value }));
          }}
        >
          <SelectTrigger className="w-[220px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={OPENCLAW_UNSET_PROFILE}>
              {t("openclaw.tools.profileUnset", {
                defaultValue: "Not set",
              })}
            </SelectItem>
            {unsupportedProfile && (
              <SelectItem
                value={OPENCLAW_UNSUPPORTED_PROFILE}
                disabled={true}
              >{`${unsupportedProfile} (${t(
                "openclaw.tools.unsupportedProfileLabel",
                {
                  defaultValue: "unsupported",
                },
              )})`}</SelectItem>
            )}
            {OPENCLAW_TOOL_PROFILES.map((profile) => (
              <SelectItem key={profile} value={profile}>
                {profileLabels[profile]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.allowList")}</Label>
        <div className="space-y-2">
          {allowList.map((item, index) => (
            <div key={item.id} className="flex items-center gap-2">
              <Input
                value={item.value}
                onChange={(e) =>
                  updateListItem(setAllowList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(setAllowList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setAllowList((prev) => [
                ...prev,
                { id: crypto.randomUUID(), value: "" },
              ]);
            }}
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addAllow")}
          </Button>
        </div>
      </div>

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.denyList")}</Label>
        <div className="space-y-2">
          {denyList.map((item, index) => (
            <div key={item.id} className="flex items-center gap-2">
              <Input
                value={item.value}
                onChange={(e) =>
                  updateListItem(setDenyList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(setDenyList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setDenyList((prev) => [
                ...prev,
                { id: crypto.randomUUID(), value: "" },
              ]);
            }}
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addDeny")}
          </Button>
        </div>
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveToolsMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveToolsMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default ToolsPanel;
