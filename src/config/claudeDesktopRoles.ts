/**
 * Claude Desktop 3P fail-all 校验接受的角色名。Desktop 1.12603.1+ 起白名单
 * 纳入 fable（app.asar 内 ["sonnet","opus","haiku","fable","mythos"]，实测
 * 2026-06-13）；此前 1.6259.1 仅接受 sonnet/opus/haiku。mythos 官方未公开
 * 发布，暂不暴露给用户。所有预设工厂、表单角色下拉、后端
 * `next_catalog_safe_route_id` 都从此映射派生 routeId，避免散落硬编码。
 */
export const CLAUDE_DESKTOP_ROLE_ROUTE_IDS = {
  sonnet: "claude-sonnet-5",
  opus: "claude-opus-4-8",
  fable: "claude-fable-5",
  haiku: "claude-haiku-4-5",
} as const;

export type ClaudeDesktopRoleId = keyof typeof CLAUDE_DESKTOP_ROLE_ROUTE_IDS;
