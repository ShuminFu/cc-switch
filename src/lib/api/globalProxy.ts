/**
 * 全局出站代理 API
 *
 * 提供获取、设置和测试全局代理的功能。
 */

import { call } from "./invoke";
import { commands } from "@/lib/bindings";
import type {
  ProxyTestResult,
  UpstreamProxyStatus,
  DetectedProxy,
} from "@/lib/bindings";
export type {
  ProxyTestResult,
  UpstreamProxyStatus,
  DetectedProxy,
} from "@/lib/bindings";

/**
 * 获取全局代理 URL
 *
 * @returns 代理 URL，null 表示未配置（直连）
 */
export async function getGlobalProxyUrl(): Promise<string | null> {
  return call(commands.getGlobalProxyUrl());
}

/**
 * 设置全局代理 URL
 *
 * @param url - 代理 URL（如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080）
 *              空字符串表示清除代理（直连）
 */
export async function setGlobalProxyUrl(url: string): Promise<void> {
  try {
    return await call(commands.setGlobalProxyUrl(url));
  } catch (error) {
    // Tauri invoke 错误可能是字符串
    throw new Error(typeof error === "string" ? error : String(error));
  }
}

/**
 * 测试代理连接
 *
 * @param url - 要测试的代理 URL
 * @returns 测试结果，包含是否成功、延迟和错误信息
 */
export async function testProxyUrl(url: string): Promise<ProxyTestResult> {
  return call(commands.testProxyUrl(url));
}

/**
 * 获取当前出站代理状态
 *
 * @returns 代理状态，包含是否启用和代理 URL
 */
export async function getUpstreamProxyStatus(): Promise<UpstreamProxyStatus> {
  return commands.getUpstreamProxyStatus();
}

/**
 * 扫描本地代理
 *
 * @returns 检测到的代理列表
 */
export async function scanLocalProxies(): Promise<DetectedProxy[]> {
  return commands.scanLocalProxies();
}
