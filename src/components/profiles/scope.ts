import type { AppId } from "@/lib/api/types";
import type { PerApp, Profile, ProfileScope } from "@/lib/api/profiles";

/**
 * 应用页 → 所属项目分组（后端 ProfileScope::for_app 的前端镜像，两处同步）
 *
 * 不在映射里的应用不支持 Profile，其标签页不渲染切换器。
 */
export const APP_PROFILE_SCOPE: Partial<Record<AppId, ProfileScope>> = {
  claude: "claude",
  "claude-desktop": "claude-desktop",
  codex: "codex",
  gemini: "gemini",
  grokbuild: "grokbuild",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
};

/** 分组内的 payload 槽位 key（后端 ProfileScope::apps 的前端镜像） */
const SCOPE_SLOT_KEYS: Record<ProfileScope, (keyof PerApp<unknown>)[]> = {
  claude: ["claude"],
  "claude-desktop": ["claude-desktop"],
  codex: ["codex"],
  gemini: ["gemini"],
  grokbuild: ["grokbuild"],
  opencode: ["opencode"],
  openclaw: ["openclaw"],
  hermes: ["hermes"],
};

/**
 * 项目在某分组是否拍过快照（任一槽位非 null 即视为拍过）
 *
 * 未拍过的项目在该分组应用时不改动配置，只绑定 current 标记。
 */
export function hasScopeSnapshot(profile: Profile, scope: ProfileScope) {
  const { providers, mcp, skills, prompts } = profile.payload;
  return SCOPE_SLOT_KEYS[scope].some(
    (app) =>
      providers[app] !== null ||
      mcp[app] !== null ||
      skills[app] !== null ||
      prompts[app] !== null,
  );
}

/**
 * What applying `profile` changes in `scope` (FE-PROV-40): the provider it
 * activates and how many MCP servers it enables, from the scope's first slot.
 * `null` means the snapshot does not touch that part.
 */
export function scopePreview(
  profile: Profile,
  scope: ProfileScope,
): { providerId: string | null; mcpCount: number | null } {
  const slot = SCOPE_SLOT_KEYS[scope][0];
  const mcp = profile.payload.mcp[slot];
  return {
    providerId: profile.payload.providers[slot] ?? null,
    mcpCount: mcp ? mcp.length : null,
  };
}
