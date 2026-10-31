import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DatabaseUpgrade } from "./components/DatabaseUpgrade";
import { UpdateProvider } from "./contexts/UpdateContext";
import "./index.css";
// 导入国际化配置
import i18n, { i18nReady } from "./i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { MotionConfig } from "framer-motion";
import { ThemeProvider } from "@/components/theme-provider";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { ErrorBoundary } from "@/app/ErrorBoundary";
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { exit } from "@tauri-apps/plugin-process";

// 根据平台添加 body class，便于平台特定样式
try {
  const ua = navigator.userAgent || "";
  const plat = (navigator.platform || "").toLowerCase();
  const isMac = /mac/i.test(ua) || plat.includes("mac");
  if (isMac) {
    document.body.classList.add("is-mac");
  }
} catch {
  // 忽略平台检测失败
}

// 配置加载错误payload类型
interface ConfigLoadErrorPayload {
  path?: string;
  error?: string;
  /** "db_version_too_new" 表示数据库版本过新，渲染应用内升级恢复界面 */
  kind?: string;
}

/**
 * 处理配置加载失败：显示错误消息并强制退出应用
 * 不给用户"取消"选项，因为配置损坏时应用无法正常运行
 */
async function handleConfigLoadError(
  payload: ConfigLoadErrorPayload | null,
): Promise<void> {
  const path = payload?.path ?? "~/.cc-switch/config.json";
  const detail = payload?.error ?? "Unknown error";

  await message(
    i18n.t("errors.configLoadFailedMessage", {
      path,
      detail,
      defaultValue:
        "Unable to read configuration file:\n{{path}}\n\nError details:\n{{detail}}\n\nPlease check if the JSON is valid, or restore from a backup file (e.g., config.json.bak) in the same directory.\n\nThe app will exit so you can fix this.",
    }),
    {
      title: i18n.t("errors.configLoadFailedTitle", {
        defaultValue: "Configuration Load Failed",
      }),
      kind: "error",
    },
  );

  await exit(1);
}

// The backend never emits a `configLoadError` event: init failures are pulled
// once via `get_init_error` in bootstrap() below (FE-ARCH-19 event contract).

async function bootstrap() {
  // Load the active locale chunk before the first render (no English flash).
  await i18nReady;

  // 启动早期主动查询后端初始化错误，避免事件竞态
  try {
    const initError = (await invoke(
      "get_init_error",
    )) as ConfigLoadErrorPayload | null;
    if (initError && initError.kind === "db_version_too_new") {
      // 数据库版本过新：渲染应用内「升级应用」恢复界面，不进入正常 App
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
            <DatabaseUpgrade payload={initError} />
            <Toaster />
          </ThemeProvider>
        </React.StrictMode>,
      );
      return;
    }
    if (initError && (initError.path || initError.error)) {
      await handleConfigLoadError(initError);
      // 注意：不会执行到这里，因为 exit(1) 会终止进程
      return;
    }
  } catch (e) {
    // 忽略拉取错误，继续渲染
    console.error("拉取初始化错误失败", e);
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <ErrorBoundary>
            <UpdateProvider>
              {/* Honour prefers-reduced-motion for every framer-motion animation (FE-UI-13). */}
              <MotionConfig reducedMotion="user">
                <App />
              </MotionConfig>
              <Toaster />
            </UpdateProvider>
          </ErrorBoundary>
        </ThemeProvider>
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

void bootstrap();
