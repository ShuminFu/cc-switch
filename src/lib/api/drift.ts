import { invoke } from "./ipc";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppId } from "./types";
import type { DriftReport, DriftResolution } from "@/shared/bindings";

/** 后端检测到 live 配置漂移时推送的事件（`services/drift/watcher.rs`） */
export const LIVE_CONFIG_DRIFT_EVENT = "live-config-drift";

export const driftApi = {
  /** 检测单个应用的 live 配置漂移（与当前供应商的预期输出比较） */
  async detect(appId: AppId): Promise<DriftReport> {
    return await invoke("get_live_config_drift", { app: appId });
  },

  /** 检测全部支持漂移检测的应用 */
  async detectAll(): Promise<DriftReport[]> {
    return await invoke("get_all_live_config_drift");
  },

  /** 修复占位符 / 并入通用配置 / 忽略，返回处理后的报告 */
  async resolve(
    appId: AppId,
    resolution: DriftResolution,
  ): Promise<DriftReport> {
    return await invoke("resolve_live_config_drift", {
      app: appId,
      resolution,
    });
  },

  async onDrift(handler: (report: DriftReport) => void): Promise<UnlistenFn> {
    return await listen<DriftReport>(LIVE_CONFIG_DRIFT_EVENT, (event) => {
      handler(event.payload);
    });
  },
};
