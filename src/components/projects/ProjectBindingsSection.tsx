import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { FolderGit2, Play, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { settingsApi } from "@/lib/api/settings";
import { projectsApi, type ProjectApplyReport } from "@/lib/api/projects";
import type { Profile } from "@/lib/api/profiles";
import { extractErrorMessage } from "@/utils/errorUtils";
import { queryKeys } from "@/lib/query/keys";

export const projectBindingsQueryKey = queryKeys.projects.bindings;

interface ProjectBindingsSectionProps {
  profiles: Profile[];
}

/**
 * 目录绑定（PRODUCT-12）：把一个目录（git 仓库根）绑定到项目，
 * 之后可以一键检测并应用；支持项目级配置的工具（Claude Code、Gemini）
 * 可选择只写入该目录内的项目配置文件，不改动全局配置。
 */
export function ProjectBindingsSection({
  profiles,
}: ProjectBindingsSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: projectBindingsQueryKey,
    queryFn: () => projectsApi.list(),
  });
  const [profileId, setProfileId] = useState<string>("");
  const [projectLevel, setProjectLevel] = useState(false);

  const bindings = data?.bindings ?? [];
  const nameOf = (id: string) =>
    profiles.find((p) => p.id === id)?.name ??
    t("projects.missingProfile", { defaultValue: "(deleted project)" });

  const onError = (error: unknown) => {
    const detail = extractErrorMessage(error) || t("common.unknown");
    toast.error(
      t("projects.actionFailed", {
        defaultValue: "Directory binding failed: {{detail}}",
        detail,
      }),
      { closeButton: true },
    );
  };

  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: projectBindingsQueryKey });

  const bindMutation = useMutation({
    mutationFn: async () => {
      const path = await settingsApi.pickDirectory();
      if (!path) return null;
      return await projectsApi.bind({ path, profileId, projectLevel });
    },
    onSuccess: async (binding) => {
      if (!binding) return;
      await refresh();
      toast.success(
        t("projects.bindSuccess", {
          defaultValue: "Bound {{path}}",
          path: binding.path,
        }),
        { closeButton: true },
      );
    },
    onError,
  });

  const unbindMutation = useMutation({
    mutationFn: (path: string) => projectsApi.unbind(path),
    onSuccess: refresh,
    onError,
  });

  const applyMutation = useMutation({
    mutationFn: (path: string) => projectsApi.apply(path),
    onSuccess: async (report: ProjectApplyReport | null) => {
      await refresh();
      await queryClient.invalidateQueries();
      if (!report) return;
      const files = [...report.writtenFiles, ...report.backups];
      const details = [...files, ...report.warnings].join("\n");
      if (report.warnings.length > 0) {
        toast.warning(
          t("projects.applyWarnings", {
            defaultValue: "Applied with {{count}} warning(s):\n{{details}}",
            count: report.warnings.length,
            details,
          }),
          { closeButton: true, duration: 10000 },
        );
      } else {
        toast.success(
          files.length > 0
            ? t("projects.applySuccessFiles", {
                defaultValue: "Applied. Updated files:\n{{details}}",
                details,
              })
            : t("projects.applySuccess", {
                defaultValue: "Directory binding applied",
              }),
          { closeButton: true },
        );
      }
    },
    onError,
  });

  const projectLevelApps = (data?.projectLevelScopes ?? [])
    .map((s) =>
      s === "claude" ? "Claude Code" : s === "gemini" ? "Gemini" : s,
    )
    .join(", ");

  return (
    <div className="space-y-2 border-t border-border-default pt-3">
      <div className="flex items-center gap-2 text-sm font-medium">
        <FolderGit2 className="h-4 w-4 opacity-70" />
        {t("projects.bindingsTitle", { defaultValue: "Directory bindings" })}
      </div>
      <p className="text-xs text-muted-foreground">
        {t("projects.bindingsDescription", {
          defaultValue:
            "Bind a folder (its git repository root) to a project, then apply it with one click. With project files on, {{apps}} get the project's provider in files inside that folder only, so other folders keep their own provider.",
          apps: projectLevelApps || "Claude Code, Gemini",
        })}
      </p>

      {bindings.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          {t("projects.noBindings", { defaultValue: "No folders bound yet" })}
        </p>
      ) : (
        <ul className="space-y-1">
          {bindings.map((binding) => (
            <li
              key={binding.path}
              className="flex items-center gap-2 rounded-md px-2 py-1 text-xs hover:bg-muted/50"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono" title={binding.path}>
                  {binding.path}
                </div>
                <div className="text-muted-foreground">
                  {nameOf(binding.profileId)}
                  {binding.projectLevel &&
                    ` · ${t("projects.projectFilesBadge", {
                      defaultValue: "project files",
                    })}`}
                </div>
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                title={t("projects.apply", { defaultValue: "Apply now" })}
                aria-label={t("projects.apply", { defaultValue: "Apply now" })}
                disabled={applyMutation.isPending}
                onClick={() => applyMutation.mutate(binding.path)}
              >
                <Play className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                title={t("projects.unbind", { defaultValue: "Remove binding" })}
                aria-label={t("projects.unbind", {
                  defaultValue: "Remove binding",
                })}
                disabled={unbindMutation.isPending}
                onClick={() => unbindMutation.mutate(binding.path)}
              >
                <Trash2 className="h-3.5 w-3.5 text-destructive" />
              </Button>
            </li>
          ))}
        </ul>
      )}

      {profiles.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <Select value={profileId} onValueChange={setProfileId}>
            <SelectTrigger
              className="h-8 w-40 text-xs"
              aria-label={t("projects.selectProfile", {
                defaultValue: "Project to bind",
              })}
            >
              <SelectValue
                placeholder={t("projects.selectProfile", {
                  defaultValue: "Project to bind",
                })}
              />
            </SelectTrigger>
            <SelectContent className="z-[200]">
              {profiles.map((profile) => (
                <SelectItem key={profile.id} value={profile.id}>
                  {profile.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <label className="flex items-center gap-1.5 text-xs">
            <Switch
              checked={projectLevel}
              onCheckedChange={setProjectLevel}
              aria-label={t("projects.projectLevel", {
                defaultValue: "Write project files",
              })}
            />
            {t("projects.projectLevel", {
              defaultValue: "Write project files",
            })}
          </label>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8"
            disabled={!profileId || bindMutation.isPending}
            onClick={() => bindMutation.mutate()}
          >
            {t("projects.bindFolder", { defaultValue: "Bind folder…" })}
          </Button>
        </div>
      )}
    </div>
  );
}
