/**
 * 供应商相关 React Query key 工厂（单一来源）
 *
 * 所有 `["providers", ...]` 形式的 key 都必须从这里取，禁止在调用处内联；
 * 需要级联刷新（live ids、OMO、OpenClaw、Hermes、用量脚本等）时使用
 * `@/entities/provider/invalidate` 中的 `invalidateProviderGraph()`。
 */

export const providerKeys = {
  /** 所有应用的供应商列表（前缀匹配） */
  all: ["providers"] as const,
  /**
   * 指定应用的供应商列表（`useProvidersQuery`）。
   * 故障转移 / 代理 API 以字符串传递 appType，因此这里接受 string。
   */
  list: (appId: string) => ["providers", appId] as const,
  /** OpenCode live 配置中的供应商 ID 列表 */
  opencodeLiveIds: ["opencodeLiveProviderIds"] as const,
  /** Claude Desktop 接管状态 */
  claudeDesktopStatus: ["claudeDesktopStatus"] as const,
};
