import type { ClaudeDesktopModelRoute } from "@/types";
import {
  CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
  type ClaudeDesktopRoleId,
} from "@/config/claudeDesktopRoles";
import type { ClaudeDesktopDefaultRoute } from "@/lib/api/providers";

/** 模型路由表中的一行（rowId 只用作 React key） */
export type RouteRow = {
  rowId: string;
  route: string;
  model: string;
  labelOverride: string;
  supports1m: boolean;
};

export type RouteRowValues = Omit<RouteRow, "rowId">;
export type RouteRole = ClaudeDesktopRoleId;

const CLAUDE_ROUTE_PREFIX = "claude-";
const ANTHROPIC_CLAUDE_ROUTE_PREFIX = "anthropic/claude-";
const LEGACY_ONE_M_MARKER = "[1m]";
const ROLE_ROUTE_IDS = CLAUDE_DESKTOP_ROLE_ROUTE_IDS;
export const ROLE_ORDER: RouteRole[] = ["sonnet", "opus", "fable", "haiku"];

export function routeRoleFromId(route: string): RouteRole {
  const normalized = route.trim().toLowerCase();
  // 与后端 claude_role_keyword 同序（opus → haiku → fable → sonnet）。
  if (normalized.includes("opus")) return "opus";
  if (normalized.includes("haiku")) return "haiku";
  if (normalized.includes("fable")) return "fable";
  return "sonnet";
}

function routeIdForRole(role: RouteRole, usedRoutes: Set<string>) {
  const baseRoute = ROLE_ROUTE_IDS[role];
  if (!usedRoutes.has(baseRoute)) return baseRoute;

  let index = 2;
  while (usedRoutes.has(`${baseRoute}-r${index}`)) {
    index += 1;
  }
  return `${baseRoute}-r${index}`;
}

function fallbackCatalogRouteId(usedRoutes: Set<string>) {
  const role = ROLE_ORDER.find((candidate) => {
    const route = ROLE_ROUTE_IDS[candidate];
    return !usedRoutes.has(route);
  });
  return routeIdForRole(role ?? "sonnet", usedRoutes);
}

export function createRouteRow(row: RouteRowValues): RouteRow {
  return {
    rowId: crypto.randomUUID(),
    ...row,
  };
}

export function isClaudeSafeRoute(route: string) {
  const normalized = route.trim().toLowerCase();
  if (normalized.includes(LEGACY_ONE_M_MARKER)) return false;
  const routeTail = normalized.startsWith(ANTHROPIC_CLAUDE_ROUTE_PREFIX)
    ? normalized.slice(ANTHROPIC_CLAUDE_ROUTE_PREFIX.length)
    : normalized.startsWith(CLAUDE_ROUTE_PREFIX)
      ? normalized.slice(CLAUDE_ROUTE_PREFIX.length)
      : "";

  // 角色前缀后必须还有实际模型标识，拒绝 claude-sonnet- 这类退化值
  // （否则会写入 profile 并触发 Claude Desktop fail-all 拒收整组）。
  // 与后端 is_claude_safe_model_id 镜像；fable 自 Desktop 1.12603.1+ 起被校验放行。
  return ["sonnet-", "opus-", "haiku-", "fable-"].some(
    (prefix) =>
      routeTail.startsWith(prefix) && routeTail.length > prefix.length,
  );
}

/** 已保存的路由表转成表单行；不安全的 route id 换成可用的角色 id，原值留作显示名 */
export function initialRouteRows(
  routes: Record<string, ClaudeDesktopModelRoute> | undefined,
): RouteRow[] {
  const usedRoutes = new Set(
    Object.keys(routes ?? {}).filter((route) => isClaudeSafeRoute(route)),
  );

  return Object.entries(routes ?? {}).map(([route, value]) => {
    const routeId = isClaudeSafeRoute(route)
      ? route
      : fallbackCatalogRouteId(usedRoutes);
    usedRoutes.add(routeId);

    return createRouteRow({
      route: routeId,
      model: value.model ?? "",
      labelOverride:
        value.labelOverride ??
        (!isClaudeSafeRoute(route) ? value.model || route : ""),
      supports1m: value.supports1m ?? false,
    });
  });
}

// Proxy 模式对齐 Claude Code：固定 Sonnet / Opus / Fable / Haiku 四档。
// 把任意来源的 route 行按角色归类到固定四槽（缺档留空），保证 UI 永远四行、
// 用户不会漏配某档导致子 agent 找不到模型。
// （fable 自 Desktop 1.12603.1+ 起被 fail-all 校验放行，可作为独立档位。）
export function normalizeProxyRows(rows: RouteRow[]): RouteRow[] {
  return ROLE_ORDER.map((role) => {
    const match = rows.find(
      (row) => row.route.trim() && routeRoleFromId(row.route) === role,
    );
    return createRouteRow({
      route: ROLE_ROUTE_IDS[role],
      model: match?.model ?? "",
      labelOverride: match?.labelOverride ?? "",
      supports1m: match?.supports1m ?? false,
    });
  });
}

export function defaultRouteRows(
  defaults: ClaudeDesktopDefaultRoute[],
  defaultModel: string,
): RouteRow[] {
  return defaults.map((route, index) =>
    createRouteRow({
      route: route.routeId,
      model: index === 0 ? defaultModel : "",
      labelOverride: "",
      supports1m: route.supports1m,
    }),
  );
}
