import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Check, Download, Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import { customToolsApi, type ToolProfileView, type ToolStatus } from "./api";

interface DraftState {
  toolId: string;
  id?: string;
  name: string;
  values: Record<string, string>;
}

const onError = (error: unknown) =>
  toast.error(extractErrorMessage(error) || String(error));

/**
 * Profiles for adjacent CLI tools (Qwen Code, Aider, user descriptors) that
 * are configured through a descriptor instead of a built-in app tab.
 */
export function CustomToolsPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<DraftState | null>(null);
  const [deleting, setDeleting] = useState<{
    toolId: string;
    profile: ToolProfileView;
  } | null>(null);
  const { data, isLoading } = useQuery({
    queryKey: queryKeys.workbench.customTools,
    queryFn: customToolsApi.list,
  });
  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: queryKeys.workbench.customTools,
    });

  const save = useMutation({
    mutationFn: (d: DraftState) =>
      customToolsApi.saveProfile(d.toolId, {
        id: d.id,
        name: d.name,
        values: d.values,
      }),
    onSuccess: async () => {
      setDraft(null);
      await refresh();
    },
    onError,
  });
  const activate = useMutation({
    mutationFn: (v: { toolId: string; profileId: string }) =>
      customToolsApi.activate(v.toolId, v.profileId),
    onSuccess: async () => {
      toast.success(
        t("workbench.tools.activated", {
          defaultValue: "Profile written to the tool's config files",
        }),
      );
      await refresh();
    },
    onError,
  });
  const importLive = useMutation({
    mutationFn: (tool: ToolStatus) =>
      customToolsApi.importLive(
        tool.descriptor.id,
        t("workbench.tools.importedName", { defaultValue: "Current config" }),
      ),
    onSuccess: refresh,
    onError,
  });
  const remove = useMutation({
    mutationFn: (v: { toolId: string; profileId: string }) =>
      customToolsApi.deleteProfile(v.toolId, v.profileId),
    onSuccess: refresh,
    onError,
    onSettled: () => setDeleting(null),
  });

  if (isLoading || !data) {
    return (
      <p className="text-sm text-muted-foreground">
        {t("common.loading", { defaultValue: "Loading…" })}
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground">
        {t("workbench.tools.description", {
          defaultValue:
            "Switch endpoint, key and model for CLI tools that do not have their own tab. Only the listed keys are changed; other settings and comments are kept, and the previous file is backed up. Add more tools by placing a JSON descriptor in {{dir}}.",
          dir: data.descriptorsDir,
        })}
      </p>
      {data.descriptorErrors.map((e) => (
        <p key={e.file} className="text-xs text-destructive" role="alert">
          {e.file}: {e.error}
        </p>
      ))}
      {data.tools.map((tool) => (
        <div
          key={tool.descriptor.id}
          className="space-y-3 rounded-lg border border-border/60 p-4"
        >
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="text-sm font-medium">{tool.descriptor.name}</h4>
            <Badge variant={tool.installed ? "default" : "secondary"}>
              {tool.installed
                ? t("workbench.tools.detected", { defaultValue: "Detected" })
                : t("workbench.tools.notDetected", {
                    defaultValue: "Not detected",
                  })}
            </Badge>
            <div className="ml-auto flex gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={() => importLive.mutate(tool)}
                disabled={importLive.isPending}
              >
                <Download className="mr-1 h-3.5 w-3.5" />
                {t("workbench.tools.importLive", {
                  defaultValue: "Import current",
                })}
              </Button>
              <Button
                size="sm"
                onClick={() =>
                  setDraft({
                    toolId: tool.descriptor.id,
                    name: "",
                    values: {},
                  })
                }
              >
                <Plus className="mr-1 h-3.5 w-3.5" />
                {t("workbench.tools.addProfile", {
                  defaultValue: "Add profile",
                })}
              </Button>
            </div>
          </div>
          <p className="break-all text-xs text-muted-foreground">
            {tool.files.join(" · ")}
          </p>
          {tool.liveError && (
            <p className="text-xs text-destructive" role="alert">
              {tool.liveError}
            </p>
          )}
          {tool.profiles.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {t("workbench.tools.noProfiles", {
                defaultValue: "No profiles yet.",
              })}
            </p>
          ) : (
            <ul className="space-y-2">
              {tool.profiles.map((p) => {
                const active = tool.activeProfile === p.id;
                return (
                  <li
                    key={p.id}
                    className="flex flex-wrap items-center gap-2 rounded-md bg-muted/40 px-3 py-2 text-sm"
                  >
                    <span className="font-medium">{p.name}</span>
                    {active && (
                      <Badge>
                        {t("workbench.tools.active", {
                          defaultValue: "Active",
                        })}
                      </Badge>
                    )}
                    <span className="truncate text-xs text-muted-foreground">
                      {[p.values.baseUrl, p.values.model]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                    <div className="ml-auto flex gap-1">
                      <Button
                        size="sm"
                        variant={active ? "secondary" : "default"}
                        onClick={() =>
                          activate.mutate({
                            toolId: tool.descriptor.id,
                            profileId: p.id,
                          })
                        }
                        disabled={activate.isPending}
                      >
                        <Check className="mr-1 h-3.5 w-3.5" />
                        {t("workbench.tools.apply", { defaultValue: "Apply" })}
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={t("workbench.tools.editProfile", {
                          defaultValue: "Edit profile",
                        })}
                        onClick={() =>
                          setDraft({
                            toolId: tool.descriptor.id,
                            id: p.id,
                            name: p.name,
                            values: { ...p.values },
                          })
                        }
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={t("workbench.tools.deleteProfile", {
                          defaultValue: "Delete profile",
                        })}
                        onClick={() =>
                          setDeleting({
                            toolId: tool.descriptor.id,
                            profile: p,
                          })
                        }
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          {draft?.toolId === tool.descriptor.id && (
            <form
              className="space-y-2 rounded-md border border-border/60 p-3"
              onSubmit={(e) => {
                e.preventDefault();
                save.mutate(draft);
              }}
            >
              <div className="space-y-1">
                <Label htmlFor={`tool-${tool.descriptor.id}-name`}>
                  {t("workbench.tools.profileName", {
                    defaultValue: "Profile name",
                  })}
                </Label>
                <Input
                  id={`tool-${tool.descriptor.id}-name`}
                  value={draft.name}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                  required
                />
              </div>
              {tool.descriptor.fields.map((f) => {
                const existing = tool.profiles.find((p) => p.id === draft.id);
                const secretKept =
                  f.secret && existing?.secretsSet.includes(f.id);
                return (
                  <div key={f.id} className="space-y-1">
                    <Label htmlFor={`tool-${tool.descriptor.id}-${f.id}`}>
                      {f.label}
                    </Label>
                    <Input
                      id={`tool-${tool.descriptor.id}-${f.id}`}
                      type={f.secret ? "password" : "text"}
                      autoComplete="off"
                      placeholder={
                        secretKept
                          ? t("workbench.tools.secretKept", {
                              defaultValue: "Saved — leave empty to keep",
                            })
                          : f.key
                      }
                      value={draft.values[f.id] ?? ""}
                      onChange={(e) => {
                        const values = { ...draft.values };
                        if (f.secret && e.target.value === "") {
                          delete values[f.id];
                        } else {
                          values[f.id] = e.target.value;
                        }
                        setDraft({ ...draft, values });
                      }}
                    />
                  </div>
                );
              })}
              <div className="flex justify-end gap-2 pt-1">
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => setDraft(null)}
                >
                  {t("common.cancel", { defaultValue: "Cancel" })}
                </Button>
                <Button type="submit" disabled={save.isPending}>
                  {t("common.save", { defaultValue: "Save" })}
                </Button>
              </div>
            </form>
          )}
        </div>
      ))}
      <ConfirmDialog
        isOpen={deleting !== null}
        title={t("workbench.tools.deleteProfile", {
          defaultValue: "Delete profile",
        })}
        message={t("workbench.tools.deleteConfirm", {
          defaultValue:
            'Delete profile "{{name}}"? The tool\'s config files are not changed.',
          name: deleting?.profile.name ?? "",
        })}
        variant="destructive"
        isPending={remove.isPending}
        onConfirm={() =>
          deleting &&
          remove.mutate({
            toolId: deleting.toolId,
            profileId: deleting.profile.id,
          })
        }
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
}
