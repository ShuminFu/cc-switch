import { QueryClient } from "@tanstack/react-query";

/**
 * staleTime 档位：查询应按数据来源显式选择，而不是依赖全局默认值。
 * - live：直接读取外部可变文件（live 配置、进程状态），每次挂载/聚焦都重新读取
 * - short：数据库支撑、所有写路径都会主动失效的数据（如供应商列表），
 *   短时间内的重复挂载/窗口聚焦直接复用缓存
 * - long：变化很少的数据（价格表、远程目录等）
 */
export const STALE_TIME = {
  live: 0,
  short: 10 * 1000,
  long: 5 * 60 * 1000,
} as const;

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: true,
      // 未显式声明 staleTime 的查询的兜底值；新查询应从 STALE_TIME 中选择档位
      staleTime: STALE_TIME.live,
    },
    mutations: {
      retry: false,
    },
  },
});
