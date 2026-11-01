import { invoke } from "@tauri-apps/api/core";
import type { ProfileScope } from "@/lib/api/profiles";
import { call } from "./invoke";
import { commands } from "@/lib/bindings";

/**
 * 目录 → 项目绑定（与后端 services/project_binding.rs 严格对应）
 */
export interface ManagedProjectFile {
  scope: ProfileScope;
  relativePath: string;
  keys: string[];
}

export interface ProjectBinding {
  /** 规范化后的绝对目录（git 仓库根，或不在仓库中的目录本身） */
  path: string;
  profileId: string;
  /** 应用时作用的分组；空 = 项目已保存过的全部分组 */
  scopes: ProfileScope[];
  /** 支持项目级配置的分组写项目内文件，而不是切换全局配置 */
  projectLevel: boolean;
  managed: ManagedProjectFile[];
  createdAt: number;
  updatedAt: number;
}

export interface ProjectBindingsResponse {
  bindings: ProjectBinding[];
  /** 支持项目级应用的分组（目前为 Claude Code 与 Gemini） */
  projectLevelScopes: ProfileScope[];
}

export interface DetectedProject {
  directory: string;
  gitRoot: string | null;
  binding: ProjectBinding | null;
  profileName: string | null;
}

export interface ProjectApplyReport {
  profileId: string;
  globalScopes: ProfileScope[];
  projectLevelScopes: ProfileScope[];
  writtenFiles: string[];
  backups: string[];
  warnings: string[];
  shouldStopProxy: boolean;
}

export const projectsApi = {
  async list(): Promise<ProjectBindingsResponse> {
    return await invoke("list_project_bindings");
  },

  async bind(options: {
    path: string;
    profileId: string;
    scopes?: ProfileScope[];
    projectLevel?: boolean;
  }): Promise<ProjectBinding> {
    return await invoke("bind_project_directory", {
      path: options.path,
      profileId: options.profileId,
      scopes: options.scopes,
      projectLevel: options.projectLevel,
    });
  },

  async unbind(path: string): Promise<boolean> {
    return await call(commands.unbindProjectDirectory(path));
  },

  /** 检测目录所属的项目绑定（不改动配置） */
  async detect(path?: string): Promise<DetectedProject> {
    return await invoke("detect_project", { path });
  },

  /** 检测并应用目录绑定；目录没有绑定时返回 null */
  async apply(path?: string): Promise<ProjectApplyReport | null> {
    return await invoke("apply_project_binding", { path });
  },
};
