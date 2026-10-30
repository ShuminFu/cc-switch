import { invoke } from "./ipc";
import type { AppId } from "./types";

/** 后端 `get_app_registry` 返回的能力布尔表 */
export interface BackendAppCaps {
  mcp: boolean;
  skills: boolean;
  prompts: boolean;
  proxyTakeover: boolean;
  tray: boolean;
  /** 请求用量进入使用统计（经代理记录或从会话日志导入） */
  usage: boolean;
  /** 会话管理中可在终端恢复会话 */
  sessionResume: boolean;
}

/** 后端应用描述符（`src-tauri/src/apps/registry.rs` 的可序列化视图） */
export interface BackendAppDescriptor {
  id: AppId;
  aliases: string[];
  labelKey: string;
  displayName: string;
  syncMode: "switch" | "additive";
  caps: BackendAppCaps;
  proxyRoute: string | null;
  inputTokenSemantics: "fresh" | "cacheInclusive";
  defaultVisible: boolean;
}

export const appsApi = {
  async getRegistry(): Promise<BackendAppDescriptor[]> {
    return invoke<BackendAppDescriptor[]>("get_app_registry");
  },
};
