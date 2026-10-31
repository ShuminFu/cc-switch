import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type {
  ProxyConfig,
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
} from "@/types/proxy";

export const proxyApi = {
  // ========== 代理服务器控制 API ==========

  // 启动代理服务器
  async startProxyServer(): Promise<ProxyServerInfo> {
    return invoke("start_proxy_server");
  },

  // 停止代理服务器（不恢复接管配置）
  async stopProxyServer(): Promise<void> {
    return call(commands.stopProxyServer());
  },

  // 停止代理服务器并恢复配置
  async stopProxyWithRestore(): Promise<void> {
    return call(commands.stopProxyWithRestore());
  },

  // 获取代理服务器状态
  async getProxyStatus(): Promise<ProxyStatus> {
    return invoke("get_proxy_status");
  },

  // 检查代理服务器是否正在运行
  async isProxyRunning(): Promise<boolean> {
    return call(commands.isProxyRunning());
  },

  // 检查是否处于接管模式
  async isLiveTakeoverActive(): Promise<boolean> {
    return call(commands.isLiveTakeoverActive());
  },

  // 代理模式下切换供应商
  async switchProxyProvider(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return call(commands.switchProxyProvider(appType, providerId));
  },

  // ========== 接管状态 API ==========

  // 获取各应用接管状态
  /** Proxy status and takeover flags in one round trip. */
  async getProxyOverview(): Promise<{
    status: ProxyStatus;
    takeover: ProxyTakeoverStatus;
  }> {
    return invoke("get_proxy_overview");
  },

  async getProxyTakeoverStatus(): Promise<ProxyTakeoverStatus> {
    return invoke("get_proxy_takeover_status");
  },

  // 为指定应用开启/关闭接管
  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    return call(commands.setProxyTakeoverForApp(appType, enabled));
  },

  // ========== Legacy 代理配置 API (兼容) ==========

  // 获取代理配置（旧版 v2 兼容接口）
  async getProxyConfig(): Promise<ProxyConfig> {
    return invoke("get_proxy_config");
  },

  // 更新代理配置（旧版 v2 兼容接口）
  async updateProxyConfig(config: ProxyConfig): Promise<void> {
    return invoke("update_proxy_config", { config });
  },

  // ========== v3+ 全局/应用级配置 API ==========

  // 获取全局代理配置
  async getGlobalProxyConfig(): Promise<GlobalProxyConfig> {
    return invoke("get_global_proxy_config");
  },

  // 更新全局代理配置
  async updateGlobalProxyConfig(config: GlobalProxyConfig): Promise<void> {
    return invoke("update_global_proxy_config", { config });
  },

  // 获取指定应用的代理配置
  async getProxyConfigForApp(appType: string): Promise<AppProxyConfig> {
    return invoke("get_proxy_config_for_app", { appType });
  },

  // 更新指定应用的代理配置
  async updateProxyConfigForApp(config: AppProxyConfig): Promise<void> {
    return invoke("update_proxy_config_for_app", { config });
  },

  // ========== 计费默认配置 API ==========

  // 获取默认成本倍率
  async getDefaultCostMultiplier(appType: string): Promise<string> {
    return call(commands.getDefaultCostMultiplier(appType));
  },

  // 设置默认成本倍率
  async setDefaultCostMultiplier(
    appType: string,
    value: string,
  ): Promise<void> {
    return call(commands.setDefaultCostMultiplier(appType, value));
  },

  // 获取计费模式来源
  async getPricingModelSource(appType: string): Promise<string> {
    return call(commands.getPricingModelSource(appType));
  },

  // 设置计费模式来源
  async setPricingModelSource(appType: string, value: string): Promise<void> {
    return call(commands.setPricingModelSource(appType, value));
  },
};
