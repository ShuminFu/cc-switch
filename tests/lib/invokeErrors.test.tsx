/**
 * Error paths with realistic Tauri rejections (TEST-26): handlers reply with
 * `tauriError(...)`, so the wrappers see exactly what the Rust backend sends
 * (a plain string, or the `AppError` payload snapshotted in
 * tests/contracts/app_error.json) and the UI shows the localized text.
 */
import type { ReactNode } from "react";
import i18n from "i18next";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http } from "msw";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import en from "@/i18n/locales/en.json";
import { invoke } from "@/lib/api/invoke";
import { providersApi } from "@/lib/api/providers";
import { BackendError, getErrorCode } from "@/lib/errors/appError";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
import { server } from "../msw/server";
import { appErrorSample, tauriError } from "../msw/tauriError";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastError(...args),
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

const respond = (command: string, response: ReturnType<typeof tauriError>) =>
  server.use(http.post(`http://tauri.local/${command}`, () => response));

beforeEach(async () => {
  i18n.addResourceBundle("en", "translation", en, true, true);
  await i18n.changeLanguage("en");
});

afterEach(() => {
  i18n.removeResourceBundle("en", "translation");
  i18n.addResourceBundle("en", "translation", {});
});

describe("invoke rejections match the Rust wire shape", () => {
  it("a String-error command rejects with the bare string", async () => {
    await expect(providersApi.switch("missing", "claude")).rejects.toBe(
      "Provider not found: missing",
    );
  });

  it("a localized AppError becomes a BackendError rendered from en.json", async () => {
    respond(
      "get_config_dir",
      tauriError({
        key: "mcp.error.commandRequired",
        en: "A stdio MCP server needs a command",
        zh: "stdio 类型的 MCP 服务器缺少 command 字段",
      }),
    );
    const err = await invoke("get_config_dir", { app: "claude" }).catch(
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(BackendError);
    expect(getErrorCode(err)).toBe("mcp.error.commandRequired");
    expect((err as Error).message).toBe("Please enter command");
  });

  it("an untranslated localized key falls back to the English half", async () => {
    const payload = appErrorSample("localized");
    respond("get_config_dir", tauriError(payload));
    const err = await invoke("get_config_dir").catch((e: unknown) => e);
    expect(getErrorCode(err)).toBe("sync.conflict.remote_changed");
    expect((err as Error).message).toBe(payload.params?.en);
  });

  it("a generic AppError uses its English template and params", async () => {
    respond("get_config_dir", tauriError(appErrorSample("config")));
    const err = await invoke("get_config_dir").catch((e: unknown) => e);
    expect(getErrorCode(err)).toBe("errors.config");
    expect((err as Error).message).toBe("Configuration error: bad value");
  });

  it("an HTTP-status AppError interpolates status and body", async () => {
    respond("get_config_dir", tauriError(appErrorSample("httpStatus")));
    const err = await invoke("get_config_dir").catch((e: unknown) => e);
    expect(getErrorCode(err)).toBe("errors.httpStatus");
    expect((err as Error).message).toBe("HTTP 429: rate limited");
  });

  it("a param-less AppError still carries its code", async () => {
    respond(
      "get_config_dir",
      tauriError(appErrorSample("allProvidersCircuitOpen")),
    );
    const err = await invoke("get_config_dir").catch((e: unknown) => e);
    expect(getErrorCode(err)).toBe("errors.allProvidersCircuitOpen");
    expect((err as Error).message).toBe(
      "All providers are circuit-broken; no provider is available",
    );
  });
});

describe("switch failure toast", () => {
  it("shows the backend's localized message in the toast description", async () => {
    respond(
      "switch_provider",
      tauriError({
        key: "mcp.error.commandRequired",
        en: "A stdio MCP server needs a command",
      }),
    );
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useSwitchProviderMutation("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync("p1").catch(() => undefined);
    });

    await waitFor(() => expect(toastError).toHaveBeenCalledTimes(1));
    expect(toastError).toHaveBeenCalledWith(
      "Switch failed",
      expect.objectContaining({
        description: "Switch failed: Please enter command",
      }),
    );
  });
});
