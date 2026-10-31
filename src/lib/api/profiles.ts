import { call } from "./invoke";
import { commands } from "@/lib/bindings";
import type * as Bindings from "@/lib/bindings";

/**
 * Profile 操作的应用分组（与后端 services/profile.rs 的 ProfileScope 严格对应）
 *
 * 项目实体全应用共享，但快照/应用/当前指针按组进行；Claude Code 与
 * Claude Desktop 的供应商独立切换，因此各自有独立分组。
 */
export type ProfileScope = "claude" | "claude-desktop" | "codex";

/**
 * 按 app 分槽的载荷容器（与后端 services/profile.rs 的 PerApp<T> 严格对应）
 */
export interface PerApp<T> {
  claude: T;
  "claude-desktop": T;
  codex: T;
}

/**
 * 项目 Profile 的配置快照（与后端 ProfilePayload 严格对应）
 *
 * 所有槽位 null = 该侧从未拍过快照（应用时不动），与"拍到的就是空集"
 * （空数组，应用时清空启用）严格区分。
 */
export interface ProfilePayload {
  providers: PerApp<string | null>;
  mcp: PerApp<string[] | null>;
  skills: PerApp<string[] | null>;
  prompts: PerApp<string | null>;
}

/**
 * A profile as returned by the backend (`ProfileDto` in the bindings). The DTO
 * types `payload` as JSON because `ProfilePayload` lives in services/profile.rs;
 * its shape is the interface above.
 */
export type Profile = Omit<Bindings.ProfileDto, "payload"> & {
  payload: ProfilePayload;
};

/** 每个分组当前激活的项目 id（未使用项目时为 null） */
export type { CurrentProfileIds } from "@/lib/bindings";

export type ProfilesResponse = Omit<Bindings.ProfilesResponse, "profiles"> & {
  profiles: Profile[];
};

function toProfile(dto: Bindings.ProfileDto): Profile {
  return { ...dto, payload: dto.payload as unknown as ProfilePayload };
}

export const profilesApi = {
  /**
   * 获取所有项目及各分组当前激活项目 id
   */
  async list(): Promise<ProfilesResponse> {
    const response = await call(commands.listProfiles());
    return { ...response, profiles: response.profiles.map(toProfile) };
  },

  /**
   * 创建新项目（只拍发起页所属分组的当前状态，其余分组槽位留空）
   */
  async create(name: string, scope: ProfileScope): Promise<Profile> {
    return toProfile(await call(commands.createProfile(name, scope)));
  },

  /**
   * 更新项目：重命名（作用于共享实体）和/或以当前状态重拍快照
   * （resnapshot 只覆盖 scope 分组的槽位，其余分组原样保留）
   */
  async update(
    id: string,
    options: { name?: string; resnapshot?: boolean; scope?: ProfileScope },
  ): Promise<Profile> {
    return toProfile(
      await call(
        commands.updateProfile(
          id,
          options.name ?? null,
          options.resnapshot ?? null,
          options.scope ?? null,
        ),
      ),
    );
  },

  /**
   * 删除项目
   */
  async delete(id: string): Promise<void> {
    return await call(commands.deleteProfile(id));
  },

  /**
   * 应用项目快照（只作用于发起页所属分组内的应用），返回 warnings
   * （best-effort，部分失败不中断）
   */
  async apply(id: string, scope: ProfileScope): Promise<string[]> {
    return await call(commands.applyProfile(id, scope));
  },

  /**
   * 不使用项目：仅清除某分组的激活标记，不改动任何配置
   */
  async clearCurrent(scope: ProfileScope): Promise<void> {
    return await call(commands.clearCurrentProfile(scope));
  },
};
