import { invoke, call, callJson } from "./invoke";
import { commands } from "@/lib/bindings";
import type { CustomEndpoint } from "@/types";
import type { AppId } from "./types";

export interface EndpointLatencyResult {
  url: string;
  latency: number | null;
  status?: number;
  error?: string;
}

export const vscodeApi = {
  async getLiveProviderSettings(appId: AppId) {
    return await callJson(commands.readLiveProviderSettings(appId));
  },

  async testApiEndpoints(
    urls: string[],
    options?: { timeoutSecs?: number },
  ): Promise<EndpointLatencyResult[]> {
    return await invoke("test_api_endpoints", {
      urls,
      timeoutSecs: options?.timeoutSecs,
    });
  },

  async getCustomEndpoints(
    appId: AppId,
    providerId: string,
  ): Promise<CustomEndpoint[]> {
    return await invoke("get_custom_endpoints", {
      app: appId,
      providerId: providerId,
    });
  },

  async addCustomEndpoint(
    appId: AppId,
    providerId: string,
    url: string,
  ): Promise<void> {
    await call(commands.addCustomEndpoint(appId, providerId, url));
  },

  async removeCustomEndpoint(
    appId: AppId,
    providerId: string,
    url: string,
  ): Promise<void> {
    await call(commands.removeCustomEndpoint(appId, providerId, url));
  },

  async updateEndpointLastUsed(
    appId: AppId,
    providerId: string,
    url: string,
  ): Promise<void> {
    await call(commands.updateEndpointLastUsed(appId, providerId, url));
  },

  async exportConfigToFile(filePath: string) {
    return await callJson(commands.exportConfigToFile(filePath));
  },

  async importConfigFromFile(filePath: string) {
    return await callJson(commands.importConfigFromFile(filePath));
  },

  async saveFileDialog(defaultName: string): Promise<string | null> {
    return await invoke("save_file_dialog", {
      defaultName,
    });
  },

  async openFileDialog(): Promise<string | null> {
    return await invoke("open_file_dialog");
  },
};
